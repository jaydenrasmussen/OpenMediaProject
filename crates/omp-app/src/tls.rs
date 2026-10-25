//! HTTPS: loading a certificate and key, and a listener for axum that
//! finishes TLS handshakes away from the accept loop (a slow client cannot
//! hold up the rest).

use std::{io, net::SocketAddr, sync::Arc, time::Duration};

use axum::serve::Listener;
use rustls::{
    pki_types::pem::PemObject,
    pki_types::{CertificateDer, PrivateKeyDer},
};

/// The TLS settings for the server, or why the files cannot be used.
pub fn server_config(cert: &str, key: &str) -> Result<Arc<rustls::ServerConfig>, String> {
    let (certs, key) = read(cert, key)?;
    // Named outright: another dependency may switch on a second provider,
    // and then rustls cannot pick a default on its own.
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("no usable TLS version: {e}"))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("the certificate and key do not go together: {e}"))?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

pub fn load_certified_key(cert: &str, key: &str) -> Result<(), String> {
    server_config(cert, key).map(|_| ())
}

fn read(
    cert: &str,
    key: &str,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), String> {
    if cert.trim().is_empty() || key.trim().is_empty() {
        return Err("a certificate file and a key file are both needed".into());
    }
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(cert)
        .map_err(|e| format!("cannot read the certificate {cert}: {e}"))?
        .collect::<Result<_, _>>()
        .map_err(|e| format!("the certificate {cert} is not valid PEM: {e}"))?;
    if certs.is_empty() {
        return Err(format!("{cert} holds no certificate"));
    }
    let key =
        PrivateKeyDer::from_pem_file(key).map_err(|e| format!("cannot read the key {key}: {e}"))?;
    Ok((certs, key))
}

use tokio::{net::TcpListener, sync::mpsc};
use tokio_rustls::{TlsAcceptor, server::TlsStream};

pub struct TlsListener {
    local: SocketAddr,
    ready: mpsc::Receiver<(TlsStream<tokio::net::TcpStream>, SocketAddr)>,
}

impl TlsListener {
    pub fn new(
        listener: TcpListener,
        config: std::sync::Arc<rustls::ServerConfig>,
    ) -> io::Result<Self> {
        let local = listener.local_addr()?;
        let (tx, ready) = mpsc::channel(64);
        let acceptor = TlsAcceptor::from(config);
        tokio::spawn(async move {
            loop {
                let (tcp, peer) = match listener.accept().await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!("accepting a connection failed: {e}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                let (acceptor, tx) = (acceptor.clone(), tx.clone());
                tokio::spawn(async move {
                    match tokio::time::timeout(Duration::from_secs(10), acceptor.accept(tcp)).await
                    {
                        Ok(Ok(tls)) => {
                            let _ = tx.send((tls, peer)).await;
                        }
                        Ok(Err(e)) => tracing::debug!(%peer, "TLS handshake failed: {e}"),
                        Err(_) => tracing::debug!(%peer, "TLS handshake timed out"),
                    }
                });
            }
        });
        Ok(Self { local, ready })
    }
}

impl Listener for TlsListener {
    type Io = TlsStream<tokio::net::TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.ready.recv().await {
            Some(c) => c,
            // The accept loop never ends while the server runs.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.local)
    }
}
