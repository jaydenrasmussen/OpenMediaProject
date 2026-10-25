//! Port, URL base, HTTPS and proxy settings, and serving over HTTPS.

use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use omp_app::{App, AppConfig, router};
use omp_db::Db;
use omp_services::{MediaProbe, NoDiskCheck, ProbeInfo};
use serde_json::{Value, json};
use tower::ServiceExt;

struct NoProbe;

#[async_trait]
impl MediaProbe for NoProbe {
    async fn probe(&self, _: &Path) -> Option<ProbeInfo> {
        None
    }
}

async fn app(dir: &Path) -> Arc<App> {
    let mut config = AppConfig::new(dir.join("data"));
    config.ui_dir = dir.join("ui");
    App::new(
        config,
        Db::in_memory().await.unwrap(),
        None,
        Arc::new(NoProbe),
        Arc::new(NoDiskCheck),
    )
    .await
    .unwrap()
}

async fn call(r: &Router, key: &str, method: Method, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri("/api/v1/config/host")
        .header("x-api-key", key);
    let body = match body {
        Some(b) => {
            req = req.header("content-type", "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let res = r.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn self_signed(dir: &Path) -> (String, String) {
    let certified =
        rcgen::generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()]).unwrap();
    let (cert, key) = (dir.join("cert.pem"), dir.join("key.pem"));
    std::fs::write(&cert, certified.cert.pem()).unwrap();
    std::fs::write(&key, certified.signing_key.serialize_pem()).unwrap();
    (cert.to_string_lossy().into(), key.to_string_lossy().into())
}

#[tokio::test]
async fn host_settings_are_checked_saved_and_never_leak_the_proxy_password() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let key = app.api_key().await.unwrap();
    let r = router(app);

    let (s, v) = call(&r, &key, Method::GET, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["port"], Value::Null);
    assert_eq!(v["running"]["port"], 8080);
    assert_eq!(v["proxy"]["enabled"], false);

    for (bad, why) in [
        (json!({ "port": 0 }), "port 0"),
        (json!({ "log": { "level": "loud" } }), "log level"),
        (json!({ "log": { "keep_days": 0 } }), "log days"),
        (json!({ "log": { "max_rows": 5 } }), "log rows"),
        (json!({ "url_base": "a b" }), "space in base"),
        (
            json!({ "proxy": { "enabled": true, "host": "" } }),
            "proxy without host",
        ),
        (
            json!({ "proxy": { "enabled": true, "host": "p", "scheme": "ftp" } }),
            "proxy type",
        ),
        (json!({ "ssl": { "enabled": true } }), "ssl without files"),
        (
            json!({ "ssl": { "enabled": true, "cert_path": "/nope.pem", "key_path": "/nope.key" } }),
            "ssl missing files",
        ),
    ] {
        assert_eq!(
            call(&r, &key, Method::PUT, Some(bad)).await.0,
            StatusCode::BAD_REQUEST,
            "{why}"
        );
    }

    let (cert, keyfile) = self_signed(dir.path());
    let (s, v) = call(
        &r,
        &key,
        Method::PUT,
        Some(json!({
            "port": 9090, "url_base": "/media/",
            "ssl": { "enabled": true, "cert_path": cert, "key_path": keyfile },
            "proxy": { "enabled": true, "scheme": "socks5", "host": "10.0.0.2", "port": 1080, "username": "me", "password": "hunter2" }
        })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        (v["port"].clone(), v["url_base"].clone()),
        (json!(9090), json!("/media"))
    );
    assert_eq!(v["proxy"]["has_password"], true);
    assert!(
        !v.to_string().contains("hunter2"),
        "the password is not sent back"
    );
    // The running server is unchanged until it restarts.
    assert_eq!(v["running"]["port"], 8080);

    assert_eq!(v["log"]["level"], "info");
    let (s, v) = call(
        &r,
        &key,
        Method::PUT,
        Some(json!({ "log": { "level": "debug", "keep_days": 3, "max_rows": 5000 } })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        (v["log"]["level"].clone(), v["log"]["keep_days"].clone()),
        (json!("debug"), json!(3))
    );

    // Saving again without a password keeps the old one.
    let (_, v) = call(
        &r,
        &key,
        Method::PUT,
        Some(json!({ "port": 9090, "url_base": "/media", "proxy": { "enabled": true, "host": "10.0.0.2", "port": 1080 } })),
    )
    .await;
    assert_eq!(v["proxy"]["has_password"], true);
}

#[tokio::test]
async fn saved_settings_apply_at_start_unless_the_environment_decides() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::in_memory().await.unwrap();
    let (cert, keyfile) = self_signed(dir.path());
    db.config()
        .set(
            "host",
            &json!({
                "port": 9191, "url_base": "/tv",
                "ssl": { "enabled": true, "cert_path": cert, "key_path": keyfile },
            })
            .to_string(),
        )
        .await
        .unwrap();

    // Not reachable from outside the crate, so go through App::open's own path.
    let mut config = AppConfig::new(dir.path().join("data"));
    omp_app::apply_host_settings(&db, &mut config).await;
    assert_eq!(config.addr.port(), 9191);
    assert_eq!(config.url_base, "/tv");
    assert!(config.tls.is_some());

    let mut pinned = AppConfig::new(dir.path().join("data"));
    pinned.addr_pinned = true;
    pinned.url_base_pinned = true;
    omp_app::apply_host_settings(&db, &mut pinned).await;
    assert_eq!(pinned.addr.port(), 8080, "OMP_ADDR wins");
    assert_eq!(pinned.url_base, "", "OMP_URL_BASE wins");
}

#[tokio::test]
async fn tmdb_uses_the_built_in_token_unless_the_owner_brings_one() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::in_memory().await.unwrap();
    let builtin = omp_metadata::tmdb::DEFAULT_READ_TOKEN;
    let started = |env: Option<&str>| {
        let mut c = AppConfig::new(dir.path().join("data"));
        c.tmdb_token = env.map(str::to_owned);
        c
    };

    let mut config = started(None);
    omp_app::apply_host_settings(&db, &mut config).await;
    assert_eq!(
        config.tmdb_token.as_deref(),
        Some(builtin),
        "nothing set: the built-in token"
    );

    db.config()
        .set(
            "host",
            &json!({ "tmdb_token": "eyJa.eyJb.sig" }).to_string(),
        )
        .await
        .unwrap();
    let mut config = started(None);
    omp_app::apply_host_settings(&db, &mut config).await;
    assert_eq!(
        config.tmdb_token.as_deref(),
        Some("eyJa.eyJb.sig"),
        "a saved token replaces it"
    );

    let mut config = started(Some("eyJenv.eyJb.sig"));
    omp_app::apply_host_settings(&db, &mut config).await;
    assert_eq!(
        config.tmdb_token.as_deref(),
        Some("eyJenv.eyJb.sig"),
        "the environment wins"
    );
}

#[tokio::test]
async fn a_tmdb_token_is_checked_kept_out_of_replies_and_can_be_reset() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let key = app.api_key().await.unwrap();
    let r = router(app);

    let (s, v) = call(
        &r,
        &key,
        Method::PUT,
        Some(json!({ "tmdb_token": "0123456789abcdef0123456789abcdef" })),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::BAD_REQUEST,
        "the short v3 key is not what is needed: {v}"
    );

    let (s, v) = call(
        &r,
        &key,
        Method::PUT,
        Some(json!({ "tmdb_token": "eyJabc.eyJdef.ghi" })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["tmdb"]["custom"], true);
    assert!(
        !v.to_string().contains("eyJabc"),
        "the token is never sent back"
    );

    // Saving something else keeps it; an empty one goes back to the built-in token.
    let (_, v) = call(&r, &key, Method::PUT, Some(json!({ "port": 9000 }))).await;
    assert_eq!(v["tmdb"]["custom"], true);
    let (_, v) = call(&r, &key, Method::PUT, Some(json!({ "tmdb_token": "" }))).await;
    assert_eq!(v["tmdb"]["custom"], false);
}

#[tokio::test]
async fn the_app_can_be_served_over_https() {
    let dir = tempfile::tempdir().unwrap();
    let (cert, keyfile) = self_signed(dir.path());
    let config = omp_app::tls::server_config(&cert, &keyfile).unwrap();

    let app = app(dir.path()).await;
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = tcp.local_addr().unwrap().port();
    let listener = {
        use axum::serve::ListenerExt;
        omp_app::tls::TlsListener::new(tcp, config)
            .unwrap()
            .tap_io(|_| {})
    };
    tokio::spawn(async move {
        axum::serve(
            listener,
            router(app).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    let health: Value = client
        .get(format!("https://127.0.0.1:{port}/api/v1/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["status"], "ok");
    // Plain HTTP to the same port goes nowhere.
    assert!(
        reqwest::get(format!("http://127.0.0.1:{port}/api/v1/health"))
            .await
            .is_err()
    );

    // A certificate that does not match its key is refused up front.
    let other = rcgen::generate_simple_self_signed(vec!["x".into()]).unwrap();
    let other_key = dir.path().join("other.key");
    std::fs::write(&other_key, other.signing_key.serialize_pem()).unwrap();
    assert!(omp_app::tls::server_config(&cert, &other_key.to_string_lossy()).is_err());
}

#[tokio::test]
async fn ui_preferences_round_trip_and_reject_unknown_choices() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let key = app.api_key().await.unwrap();
    let r = router(app);
    let send = |method: Method, body: Option<Value>| {
        let r = r.clone();
        let key = key.clone();
        async move {
            let mut req = Request::builder()
                .method(method)
                .uri("/api/v1/config/ui")
                .header("x-api-key", key);
            let body = match body {
                Some(b) => {
                    req = req.header("content-type", "application/json");
                    Body::from(b.to_string())
                }
                None => Body::empty(),
            };
            let res = r.oneshot(req.body(body).unwrap()).await.unwrap();
            let status = res.status();
            let bytes = res.into_body().collect().await.unwrap().to_bytes();
            (
                status,
                serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
            )
        }
    };
    let (_, v) = send(Method::GET, None).await;
    assert_eq!(
        (
            v["language"].clone(),
            v["theme"].clone(),
            v["date_format"].clone()
        ),
        (json!("en"), json!("auto"), json!("locale"))
    );
    let (s, v) = send(
        Method::PUT,
        Some(json!({ "theme": "dark", "date_format": "iso", "time_format": "24h" })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, v) = send(Method::GET, None).await;
    assert_eq!(
        (
            v["theme"].clone(),
            v["date_format"].clone(),
            v["time_format"].clone(),
            v["language"].clone()
        ),
        (json!("dark"), json!("iso"), json!("24h"), json!("en"))
    );
    for bad in [
        json!({ "theme": "neon" }),
        json!({ "language": "tlh" }),
        json!({ "date_format": "x" }),
    ] {
        assert_eq!(
            send(Method::PUT, Some(bad)).await.0,
            StatusCode::BAD_REQUEST
        );
    }
}

/// A minimal SMTP server: accepts one message and returns what was sent.
async fn fake_smtp() -> (u16, tokio::task::JoinHandle<String>) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let (r, mut w) = sock.into_split();
        let mut r = BufReader::new(r);
        w.write_all(b"220 test ESMTP\r\n").await.unwrap();
        let (mut seen, mut in_data) = (String::new(), false);
        let mut line = String::new();
        loop {
            line.clear();
            if r.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            seen.push_str(&line);
            if in_data {
                if line == ".\r\n" {
                    in_data = false;
                    w.write_all(b"250 queued\r\n").await.unwrap();
                }
                continue;
            }
            let cmd = line.to_uppercase();
            let reply: &[u8] = if cmd.starts_with("EHLO") {
                b"250 test\r\n"
            } else if cmd.starts_with("DATA") {
                in_data = true;
                b"354 go\r\n"
            } else if cmd.starts_with("QUIT") {
                w.write_all(b"221 bye\r\n").await.unwrap();
                break;
            } else {
                b"250 ok\r\n"
            };
            w.write_all(reply).await.unwrap();
        }
        seen
    });
    (port, task)
}

#[tokio::test]
async fn email_notifications_are_checked_and_sent_over_smtp() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let key = app.api_key().await.unwrap();
    let r = router(app);
    let post = |path: &'static str, body: Value| {
        let (r, key) = (r.clone(), key.clone());
        async move {
            let req = Request::builder()
                .method(Method::POST)
                .uri(path)
                .header("x-api-key", key)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap();
            let res = r.oneshot(req).await.unwrap();
            let status = res.status();
            let bytes = res.into_body().collect().await.unwrap().to_bytes();
            (
                status,
                serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
            )
        }
    };
    let mail = |port: u16, to: &str| {
        json!({ "name": "Mail", "implementation": "email", "settings": { "triggers": ["grab"], "settings": {
            "server": "127.0.0.1", "port": port, "security": "none",
            "from": "omp@example.com", "to": to } } })
    };
    let (s, _) = post("/api/v1/notification/test", mail(25, "not an address")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (port, smtp) = fake_smtp().await;
    let (s, v) = post(
        "/api/v1/notification/test",
        mail(port, "me@example.com, you@example.com"),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let seen = tokio::time::timeout(std::time::Duration::from_secs(5), smtp)
        .await
        .unwrap()
        .unwrap();
    assert!(
        seen.contains("RCPT TO:<me@example.com>") && seen.contains("RCPT TO:<you@example.com>"),
        "{seen}"
    );
    assert!(
        seen.contains("Subject: OpenMediaProject: Test notification"),
        "{seen}"
    );
    assert!(seen.contains("If you can read this"), "{seen}");
}
