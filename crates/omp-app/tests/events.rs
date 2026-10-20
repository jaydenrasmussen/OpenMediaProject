//! The live-update WebSocket against a real listening server.

use std::{path::Path, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures_util::StreamExt;
use omp_app::{App, AppConfig, router};
use omp_db::Db;
use omp_services::{MediaProbe, NoDiskCheck, ProbeInfo};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue};

struct NoProbe;

#[async_trait]
impl MediaProbe for NoProbe {
    async fn probe(&self, _: &Path) -> Option<ProbeInfo> {
        None
    }
}

async fn start() -> (String, String, Arc<App>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(
        AppConfig::new(dir.path().join("data")),
        Db::in_memory().await.unwrap(),
        None,
        Arc::new(NoProbe),
        Arc::new(NoDiskCheck),
    )
    .await
    .unwrap();
    let key = app.api_key().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let r = router(app.clone());
    tokio::spawn(async move { axum::serve(listener, r).await.unwrap() });
    (addr.to_string(), key, app, dir)
}

fn request(addr: &str, key: Option<&str>, origin: Option<&str>) -> impl IntoClientRequest {
    let mut req = format!("ws://{addr}/api/v1/ws")
        .into_client_request()
        .unwrap();
    if let Some(k) = key {
        req.headers_mut()
            .insert("x-api-key", HeaderValue::from_str(k).unwrap());
    }
    if let Some(o) = origin {
        req.headers_mut()
            .insert("origin", HeaderValue::from_str(o).unwrap());
    }
    req
}

#[tokio::test]
async fn a_change_through_the_api_reaches_connected_clients() {
    let (addr, key, _app, _dir) = start().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(request(&addr, Some(&key), None))
        .await
        .unwrap();

    let client = reqwest::Client::new();
    let res = client
        .post(format!("http://{addr}/api/v1/tag"))
        .header("x-api-key", &key)
        .header("content-type", "application/json")
        .body(r#"{"label":"kids"}"#)
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());

    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("an event arrives")
        .unwrap()
        .unwrap();
    assert_eq!(msg.into_text().unwrap().as_str(), r#"{"kind":"changed"}"#);

    // Reading does not announce anything.
    client
        .get(format!("http://{addr}/api/v1/tag"))
        .header("x-api-key", &key)
        .send()
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(400), ws.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn the_socket_needs_authentication_and_a_same_origin_page() {
    let (addr, key, _app, _dir) = start().await;
    assert!(
        tokio_tungstenite::connect_async(request(&addr, None, None))
            .await
            .is_err(),
        "no credentials"
    );
    assert!(
        tokio_tungstenite::connect_async(request(&addr, Some(&key), Some("https://evil.example")))
            .await
            .is_err(),
        "another site"
    );
    assert!(
        tokio_tungstenite::connect_async(request(
            &addr,
            Some(&key),
            Some(&format!("http://{addr}"))
        ))
        .await
        .is_ok()
    );
}
