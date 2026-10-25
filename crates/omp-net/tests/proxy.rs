use omp_net::{Proxy, client_builder, set_proxy};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A stand-in HTTP proxy that answers every request and reports what it saw.
async fn fake_proxy() -> (u16, tokio::sync::mpsc::UnboundedReceiver<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let (mut conn, _) = listener.accept().await.unwrap();
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = conn.read(&mut buf).await.unwrap_or(0);
                let _ = tx.send(String::from_utf8_lossy(&buf[..n]).into_owned());
                let _ = conn
                    .write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
                    )
                    .await;
            });
        }
    });
    (port, rx)
}

#[tokio::test]
async fn requests_go_through_the_proxy_except_for_bypassed_hosts() {
    let (port, mut seen) = fake_proxy().await;
    set_proxy(Some(Proxy {
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port,
        username: Some("me".into()),
        password: Some("secret".into()),
        bypass: vec!["direct.invalid".into()],
    }));
    let client = client_builder().build().unwrap();

    let body = client
        .get("http://far-away.invalid/x?y=1")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(body, "ok");
    let request = seen.recv().await.unwrap();
    assert!(
        request.starts_with("GET http://far-away.invalid/x?y=1 HTTP/1.1"),
        "{request}"
    );
    assert!(
        request
            .to_lowercase()
            .contains("proxy-authorization: basic"),
        "{request}"
    );
    assert!(request.contains("OpenMediaProject/"), "our user agent");

    // A bypassed host is not sent to the proxy (and so fails to resolve here).
    assert!(client.get("http://direct.invalid/").send().await.is_err());
    assert!(seen.try_recv().is_err(), "the proxy never saw it");

    // Without a proxy, nothing goes to it either.
    set_proxy(None);
    let direct = client_builder().build().unwrap();
    assert!(direct.get("http://far-away.invalid/").send().await.is_err());
    assert!(seen.try_recv().is_err());
}
