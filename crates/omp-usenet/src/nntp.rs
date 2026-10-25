//! A minimal NNTP client: connect (optionally over TLS), authenticate, and
//! fetch article bodies by message-id.

use std::{sync::Arc, time::Duration};

use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::timeout,
};

#[derive(Debug, thiserror::Error)]
pub enum NntpError {
    /// 430: the server does not have the article.
    #[error("article not found")]
    ArticleNotFound,
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("server said: {0}")]
    Protocol(String),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("timed out")]
    Timeout,
}

impl NntpError {
    /// Worth retrying on a fresh connection.
    pub fn is_transient(&self) -> bool {
        matches!(self, NntpError::Io(_) | NntpError::Timeout)
    }
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub username: Option<String>,
    pub password: Option<String>,
    pub max_connections: usize,
    pub timeout: Duration,
    /// Lower is tried first; higher numbers are backups for missing articles.
    pub priority: u32,
    /// Extra trusted CA or server certificates (DER), for private servers.
    pub trusted_certs: Vec<Vec<u8>>,
}

impl ServerConfig {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            tls: false,
            username: None,
            password: None,
            max_connections: 8,
            timeout: Duration::from_secs(30),
            priority: 0,
            trusted_certs: Vec::new(),
        }
    }
}

trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

pub struct NntpConnection {
    stream: BufReader<Box<dyn Stream>>,
    timeout: Duration,
}

fn tls_config(extra: &[Vec<u8>]) -> Arc<tokio_rustls::rustls::ClientConfig> {
    let mut roots = tokio_rustls::rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    for der in extra {
        if let Err(e) = roots.add(tokio_rustls::rustls::pki_types::CertificateDer::from(
            der.clone(),
        )) {
            tracing::warn!("ignoring unusable trusted certificate: {e}");
        }
    }
    Arc::new(
        tokio_rustls::rustls::ClientConfig::builder_with_provider(Arc::new(
            tokio_rustls::rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("ring supports the default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth(),
    )
}

impl NntpConnection {
    pub async fn connect(config: &ServerConfig) -> Result<Self, NntpError> {
        let tcp = timeout(
            config.timeout,
            TcpStream::connect((config.host.as_str(), config.port)),
        )
        .await
        .map_err(|_| NntpError::Timeout)??;
        tcp.set_nodelay(true).ok();
        let stream: Box<dyn Stream> = if config.tls {
            let name = tokio_rustls::rustls::pki_types::ServerName::try_from(config.host.clone())
                .map_err(|e| NntpError::Protocol(format!("bad host name: {e}")))?;
            let connector = tokio_rustls::TlsConnector::from(tls_config(&config.trusted_certs));
            Box::new(
                timeout(config.timeout, connector.connect(name, tcp))
                    .await
                    .map_err(|_| NntpError::Timeout)??,
            )
        } else {
            Box::new(tcp)
        };
        let mut conn = Self {
            stream: BufReader::new(stream),
            timeout: config.timeout,
        };

        let (code, text) = conn.read_status().await?;
        if code != 200 && code != 201 {
            return Err(NntpError::Protocol(format!("{code} {text}")));
        }
        if let Some(user) = &config.username {
            conn.authenticate(user, config.password.as_deref().unwrap_or(""))
                .await?;
        }
        Ok(conn)
    }

    async fn read_line(&mut self) -> Result<Vec<u8>, NntpError> {
        let mut line = Vec::new();
        let n = timeout(self.timeout, self.stream.read_until(b'\n', &mut line))
            .await
            .map_err(|_| NntpError::Timeout)??;
        if n == 0 {
            return Err(NntpError::Io(std::io::ErrorKind::UnexpectedEof.into()));
        }
        Ok(line)
    }

    async fn read_status(&mut self) -> Result<(u16, String), NntpError> {
        let line = self.read_line().await?;
        let text = String::from_utf8_lossy(&line).trim_end().to_owned();
        let code = text
            .get(..3)
            .and_then(|c| c.parse().ok())
            .ok_or_else(|| NntpError::Protocol(format!("unreadable reply: {text}")))?;
        Ok((code, text.get(4..).unwrap_or("").to_owned()))
    }

    async fn send(&mut self, command: &str) -> Result<(), NntpError> {
        let stream = self.stream.get_mut();
        timeout(self.timeout, async {
            stream.write_all(command.as_bytes()).await?;
            stream.write_all(b"\r\n").await?;
            stream.flush().await
        })
        .await
        .map_err(|_| NntpError::Timeout)??;
        Ok(())
    }

    async fn authenticate(&mut self, user: &str, pass: &str) -> Result<(), NntpError> {
        self.send(&format!("AUTHINFO USER {user}")).await?;
        let (code, text) = self.read_status().await?;
        match code {
            281 => return Ok(()),
            381 => {}
            _ => return Err(NntpError::Auth(format!("{code} {text}"))),
        }
        self.send(&format!("AUTHINFO PASS {pass}")).await?;
        let (code, text) = self.read_status().await?;
        if code == 281 {
            Ok(())
        } else {
            Err(NntpError::Auth(format!("{code} {text}")))
        }
    }

    /// Fetches an article body. Dot-stuffing is removed; lines keep `\r\n`.
    pub async fn body(&mut self, message_id: &str) -> Result<Vec<u8>, NntpError> {
        self.send(&format!("BODY <{}>", message_id.trim_matches(['<', '>'])))
            .await?;
        let (code, text) = self.read_status().await?;
        match code {
            222 => {}
            430 | 423 | 420 => return Err(NntpError::ArticleNotFound),
            480 => return Err(NntpError::Auth(text)),
            _ => return Err(NntpError::Protocol(format!("{code} {text}"))),
        }
        let mut body = Vec::new();
        loop {
            let line = self.read_line().await?;
            let trimmed = line
                .strip_suffix(b"\r\n")
                .or_else(|| line.strip_suffix(b"\n"))
                .unwrap_or(&line);
            if trimmed == b"." {
                return Ok(body);
            }
            let content = trimmed
                .strip_prefix(b"..")
                .map_or(trimmed, |_| &trimmed[1..]);
            body.extend_from_slice(content);
            body.extend_from_slice(b"\r\n");
        }
    }

    pub async fn quit(mut self) {
        let _ = self.send("QUIT").await;
    }
}

#[cfg(test)]
pub(crate) mod mock {
    //! A scriptable NNTP server for tests.
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::TcpListener,
    };

    #[derive(Default)]
    pub struct MockState {
        pub articles: HashMap<String, Vec<u8>>,
        pub require_auth: Option<(String, String)>,
        pub log: Vec<String>,
        pub connections: usize,
    }

    pub async fn spawn(state: Arc<Mutex<MockState>>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    return;
                };
                let state = state.clone();
                state.lock().unwrap().connections += 1;
                tokio::spawn(async move {
                    let (r, mut w) = socket.into_split();
                    let mut r = BufReader::new(r);
                    w.write_all(b"200 mock ready\r\n").await.unwrap();
                    let mut user = None;
                    let mut authed = state.lock().unwrap().require_auth.is_none();
                    let mut line = String::new();
                    loop {
                        line.clear();
                        if r.read_line(&mut line).await.unwrap_or(0) == 0 {
                            return;
                        }
                        let cmd = line.trim_end().to_owned();
                        state.lock().unwrap().log.push(cmd.clone());
                        let reply: Vec<u8> =
                            if let Some(u) = cmd.strip_prefix("AUTHINFO USER ") {
                                user = Some(u.to_owned());
                                b"381 password please\r\n".to_vec()
                            } else if let Some(p) = cmd.strip_prefix("AUTHINFO PASS ") {
                                let ok =
                                    state.lock().unwrap().require_auth.as_ref().is_some_and(
                                        |(eu, ep)| user.as_deref() == Some(eu) && p == ep,
                                    );
                                authed = ok;
                                if ok {
                                    b"281 welcome\r\n".to_vec()
                                } else {
                                    b"481 nope\r\n".to_vec()
                                }
                            } else if let Some(id) = cmd.strip_prefix("BODY ") {
                                let id = id.trim_matches(['<', '>']);
                                if !authed {
                                    b"480 authentication required\r\n".to_vec()
                                } else if let Some(body) = state.lock().unwrap().articles.get(id) {
                                    let mut out = b"222 0 <x> body follows\r\n".to_vec();
                                    for l in body.split(|&b| b == b'\n') {
                                        let l = l.strip_suffix(b"\r").unwrap_or(l);
                                        if l.is_empty()
                                            && std::ptr::eq(l.as_ptr(), body[body.len()..].as_ptr())
                                        {
                                            continue;
                                        }
                                        if l.first() == Some(&b'.') {
                                            out.push(b'.');
                                        }
                                        out.extend_from_slice(l);
                                        out.extend_from_slice(b"\r\n");
                                    }
                                    out.extend_from_slice(b".\r\n");
                                    out
                                } else {
                                    b"430 no such article\r\n".to_vec()
                                }
                            } else if cmd == "QUIT" {
                                let _ = w.write_all(b"205 bye\r\n").await;
                                return;
                            } else {
                                b"500 what?\r\n".to_vec()
                            };
                        w.write_all(&reply).await.unwrap();
                    }
                });
            }
        });
        port
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{mock::*, *};

    fn config(port: u16) -> ServerConfig {
        ServerConfig::new("127.0.0.1", port)
    }

    #[tokio::test]
    async fn fetches_a_body_and_unstuffs_dots() {
        let state = Arc::new(Mutex::new(MockState::default()));
        state.lock().unwrap().articles.insert(
            "a@b".into(),
            b".leading dot\r\nplain\r\n..two dots\r\n".to_vec(),
        );
        let port = spawn(state.clone()).await;

        let mut c = NntpConnection::connect(&config(port)).await.unwrap();
        let body = c.body("a@b").await.unwrap();
        assert_eq!(body, b".leading dot\r\nplain\r\n..two dots\r\n");
        assert!(matches!(
            c.body("missing@x").await,
            Err(NntpError::ArticleNotFound)
        ));
        // The connection is still good after a 430.
        assert_eq!(c.body("<a@b>").await.unwrap(), body);
        c.quit().await;
        assert!(state.lock().unwrap().log.iter().any(|l| l == "BODY <a@b>"));
    }

    #[tokio::test]
    async fn authenticates_when_asked() {
        let state = Arc::new(Mutex::new(MockState::default()));
        state.lock().unwrap().require_auth = Some(("me".into(), "secret".into()));
        state
            .lock()
            .unwrap()
            .articles
            .insert("a@b".into(), b"x\r\n".to_vec());
        let port = spawn(state).await;

        let mut ok = config(port);
        ok.username = Some("me".into());
        ok.password = Some("secret".into());
        assert_eq!(
            NntpConnection::connect(&ok)
                .await
                .unwrap()
                .body("a@b")
                .await
                .unwrap(),
            b"x\r\n"
        );

        let mut bad = ok.clone();
        bad.password = Some("wrong".into());
        assert!(matches!(
            NntpConnection::connect(&bad).await,
            Err(NntpError::Auth(_))
        ));

        // No credentials against a server that wants them: the first BODY says so.
        let mut anon = NntpConnection::connect(&config(port)).await.unwrap();
        assert!(matches!(anon.body("a@b").await, Err(NntpError::Auth(_))));
    }

    #[tokio::test]
    async fn works_over_tls_and_rejects_untrusted_servers() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio_rustls::rustls::{self, pki_types::PrivateKeyDer};

        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
        let der = cert.cert.der().to_vec();
        let key = PrivateKeyDer::try_from(cert.key_pair.serialize_der()).unwrap();
        let server_config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.cert.der().clone()], key)
        .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else {
                    return;
                };
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    let Ok(tls) = acceptor.accept(tcp).await else {
                        return;
                    };
                    let (r, mut w) = tokio::io::split(tls);
                    let mut r = BufReader::new(r);
                    w.write_all(b"200 secure\r\n").await.unwrap();
                    let mut line = String::new();
                    while r.read_line(&mut line).await.unwrap_or(0) > 0 {
                        if line.starts_with("BODY") {
                            w.write_all(b"222 ok\r\nsecret data\r\n.\r\n")
                                .await
                                .unwrap();
                        }
                        line.clear();
                    }
                });
            }
        });

        let mut cfg = ServerConfig::new("localhost", port);
        cfg.tls = true;
        cfg.timeout = Duration::from_secs(5);
        assert!(
            NntpConnection::connect(&cfg).await.is_err(),
            "self-signed cert is not trusted by default"
        );

        cfg.trusted_certs = vec![der];
        let mut c = NntpConnection::connect(&cfg).await.unwrap();
        assert_eq!(c.body("x@y").await.unwrap(), b"secret data\r\n");
    }

    #[tokio::test]
    async fn connection_failures_are_transient_errors() {
        let mut cfg = config(1); // nothing listens on port 1
        cfg.timeout = Duration::from_secs(2);
        let err = NntpConnection::connect(&cfg)
            .await
            .err()
            .expect("connect fails");
        assert!(err.is_transient(), "{err}");
    }
}
