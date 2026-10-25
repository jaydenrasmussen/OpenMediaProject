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

async fn start_with_connect_info() -> (String, String, Arc<App>, tempfile::TempDir) {
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
    tokio::spawn(async move {
        axum::serve(
            listener,
            r.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap()
    });
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

#[tokio::test]
async fn every_described_route_exists_and_is_protected() {
    let (addr, key, _app, _dir) = start().await;
    let client = reqwest::Client::new();
    let spec: serde_json::Value = client
        .get(format!("http://{addr}/api/v1/openapi.json"))
        .header("x-api-key", &key)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(spec["openapi"], "3.0.3");
    let public = ["/health", "/auth/status", "/auth/setup", "/auth/login"];
    for (path, item) in spec["paths"].as_object().unwrap() {
        for method in item.as_object().unwrap().keys() {
            let concrete = path
                .replace("{id}", "1")
                .replace("{season}", "1")
                .replace(['{', '}'], "");
            let m = reqwest::Method::from_bytes(method.to_uppercase().as_bytes()).unwrap();
            let res = client
                .request(m, format!("http://{addr}/api/v1{concrete}"))
                .send()
                .await
                .unwrap();
            let status = res.status().as_u16();
            if public.contains(&path.as_str()) {
                assert_ne!(status, 404, "{method} {path}");
            } else {
                assert_eq!(
                    status, 401,
                    "{method} {path}: a described route must exist and need a login"
                );
            }
        }
    }
}

#[tokio::test]
async fn pictures_are_cached_and_only_from_allowed_hosts() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    let origin = axum::Router::new()
        .route(
            "/p.jpg",
            axum::routing::get(move || {
                h.fetch_add(1, Ordering::SeqCst);
                async {
                    (
                        [(axum::http::header::CONTENT_TYPE, "image/jpeg")],
                        vec![0xFFu8, 0xD8, 1, 2, 3],
                    )
                }
            }),
        )
        .route(
            "/page.html",
            axum::routing::get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "text/html")],
                    "<b>no</b>",
                )
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin_addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, origin).await.unwrap() });

    let dir = tempfile::tempdir().unwrap();
    let mut config = AppConfig::new(dir.path().join("data"));
    config.image_hosts = vec!["127.0.0.1".into()];
    let app = App::new(
        config,
        Db::in_memory().await.unwrap(),
        None,
        Arc::new(NoProbe),
        Arc::new(NoDiskCheck),
    )
    .await
    .unwrap();
    let key = app.api_key().await.unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let r = router(app.clone());
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });

    let client = reqwest::Client::new();
    let get = |u: String| {
        let client = client.clone();
        let key = key.clone();
        async move {
            client
                .get(format!("http://{addr}/api/v1/image"))
                .query(&[("url", u)])
                .header("x-api-key", key)
                .send()
                .await
                .unwrap()
        }
    };
    let picture = format!("http://{origin_addr}/p.jpg");
    for _ in 0..2 {
        let res = get(picture.clone()).await;
        assert_eq!(res.status(), 200);
        assert_eq!(res.headers()["content-type"], "image/jpeg");
        assert_eq!(res.bytes().await.unwrap().as_ref(), [0xFF, 0xD8, 1, 2, 3]);
    }
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "the second request came from disk"
    );
    assert_eq!(
        get(format!("http://{origin_addr}/page.html"))
            .await
            .status(),
        502,
        "not a picture"
    );
    assert_eq!(
        get("http://example.com/p.jpg".into()).await.status(),
        400,
        "host not allowed"
    );
    assert_eq!(get("file:///etc/passwd".into()).await.status(), 400);
}

#[tokio::test]
async fn the_local_network_can_skip_signing_in_only_when_allowed() {
    let (addr, key, app, _dir) = start_with_connect_info().await;
    let client = reqwest::Client::new();
    let url = |p: &str| format!("http://{addr}/api/v1{p}");
    // An account must exist; the API key stands in for the owner.
    let status = |headers: &[(&str, &str)]| {
        let mut r = client.get(url("/queue"));
        for (k, v) in headers {
            r = r.header(*k, *v);
        }
        async move { r.send().await.unwrap().status().as_u16() }
    };
    assert_eq!(status(&[]).await, 401);
    // No account yet, so the switch is refused.
    let put = |on: bool| {
        client
            .put(url("/config/security"))
            .header("x-api-key", &key)
            .header("content-type", "application/json")
            .body(format!(r#"{{"disable_auth_for_local":{on}}}"#))
            .send()
    };
    assert_eq!(put(true).await.unwrap().status(), 400);
    app.db.users().create_first("owner", "hash").await.unwrap();
    assert_eq!(put(true).await.unwrap().status(), 200);
    assert_eq!(status(&[]).await, 200, "127.0.0.1 is local");
    assert_eq!(
        status(&[("x-forwarded-for", "8.8.8.8")]).await,
        401,
        "behind a proxy it never counts"
    );
    assert_eq!(put(false).await.unwrap().status(), 200);
    assert_eq!(status(&[]).await, 401);
}
