//! Plugins end to end: a real .wasm notification plugin talking to a local
//! HTTP server, driven through the API and the history dispatcher.

use std::{path::PathBuf, process::Command, sync::Arc};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use omp_app::{App, AppConfig, router};
use omp_core::{history::HistoryEvent, movie::MovieStatus, quality::QualityModel};
use omp_db::{
    Db,
    repo::{HistoryRecord, NewMovie},
};
use omp_services::{MediaProbe, NoDiskCheck, ProbeInfo};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
};
use tower::ServiceExt;

struct NoProbe;

#[async_trait]
impl MediaProbe for NoProbe {
    async fn probe(&self, _: &std::path::Path) -> Option<ProbeInfo> {
        None
    }
}

/// Builds `plugins/webhook` and lays it out the way the app expects.
fn install_webhook(into: &std::path::Path) {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/webhook");
    let cargo = String::from_utf8(
        Command::new("rustup")
            .args(["which", "cargo"])
            .output()
            .expect("rustup is required to build plugins")
            .stdout,
    )
    .unwrap();
    let bin = PathBuf::from(cargo.trim()).parent().unwrap().to_path_buf();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let status = Command::new(bin.join("cargo"))
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .current_dir(&source)
        .env("PATH", path)
        .status()
        .unwrap();
    assert!(status.success());
    let dest = into.join("webhook");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::copy(source.join("plugin.toml"), dest.join("plugin.toml")).unwrap();
    std::fs::copy(
        source.join("target/wasm32-wasip2/release/webhook.wasm"),
        dest.join("webhook.wasm"),
    )
    .unwrap();
}

/// Accepts connections, answers 200 and sends each request body down the channel.
async fn capture_server() -> (String, mpsc::UnboundedReceiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut data = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    data.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&data).into_owned();
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let length = head
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if body.len() >= length {
                            let _ = tx.send(body.to_owned());
                            let _ = socket
                                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                                .await;
                            return;
                        }
                    }
                }
            });
        }
    });
    (url, rx)
}

struct Rig {
    app: Arc<App>,
    router: Router,
    key: String,
    _dir: tempfile::TempDir,
}

async fn rig(with_plugin: bool) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut config = AppConfig::new(dir.path().join("data"));
    let plugin_dir = dir.path().join("plugins");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    if with_plugin {
        install_webhook(&plugin_dir);
    }
    config.plugin_dirs = vec![plugin_dir];
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
    Rig {
        router: router(app.clone()),
        app,
        key,
        _dir: dir,
    }
}

async fn call(r: &Rig, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("x-api-key", &r.key);
    let body = match body {
        Some(b) => {
            req = req.header("content-type", "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let res = r
        .router
        .clone()
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn plugins_are_listed_and_describe_their_settings() {
    let r = rig(true).await;
    let (s, list) = call(&r, Method::GET, "/api/v1/plugin", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(list["plugins"][0]["id"], "webhook");
    assert_eq!(list["plugins"][0]["kind"], "notification");
    assert_eq!(list["problems"], json!([]));
    let (s, schema) = call(&r, Method::GET, "/api/v1/plugin/webhook/schema", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(schema["required"], json!(["url"]));
    assert_eq!(
        call(&r, Method::GET, "/api/v1/plugin/nope/schema", None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_broken_plugin_is_reported_not_fatal() {
    let r = rig(false).await;
    let dir = r.app.config.plugin_dirs[0].join("bad");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("plugin.toml"), "nonsense").unwrap();
    // Plugins load at startup, so a second app over the same folder sees it.
    let mut config = r.app.config.clone();
    config.data_dir = r._dir.path().join("data2");
    let again = App::new(
        config,
        Db::in_memory().await.unwrap(),
        None,
        Arc::new(NoProbe),
        Arc::new(NoDiskCheck),
    )
    .await
    .unwrap();
    let health = omp_app::router(again.clone());
    let key = again.api_key().await.unwrap();
    let res = health
        .oneshot(
            Request::builder()
                .uri("/api/v1/system/health")
                .header("x-api-key", key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let issues: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        issues
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["source"] == "Plugins"),
        "{issues}"
    );
}

#[tokio::test]
async fn notifications_validate_test_and_follow_history() {
    let r = rig(true).await;
    let (url, mut received) = capture_server().await;

    // Unknown plugin, unknown event: refused.
    for body in [
        json!({ "name": "x", "implementation": "nope", "plugin_id": "nope", "settings": {} }),
        json!({ "name": "x", "implementation": "webhook", "plugin_id": "webhook",
                "settings": { "triggers": ["explode"], "settings": { "url": url } } }),
        json!({ "name": "x", "implementation": "webhook", "settings": {} }),
    ] {
        assert_eq!(
            call(&r, Method::POST, "/api/v1/notification", Some(body))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }

    let hook = json!({
        "name": "Hook", "implementation": "webhook", "plugin_id": "webhook",
        "settings": { "triggers": ["grab", "download-failed"], "settings": { "url": url } },
    });
    let (s, tested) = call(
        &r,
        Method::POST,
        "/api/v1/notification/test",
        Some(hook.clone()),
    )
    .await;
    assert_eq!(
        (s, tested["ok"].clone()),
        (StatusCode::OK, json!(true)),
        "{tested}"
    );
    let body: Value = serde_json::from_str(&received.recv().await.unwrap()).unwrap();
    assert_eq!(body["event"], "test");

    // A bad URL fails the test with the plugin's message.
    let mut broken = hook.clone();
    broken["settings"]["settings"]["url"] = json!("not a url");
    let (_, tested) = call(&r, Method::POST, "/api/v1/notification/test", Some(broken)).await;
    assert_eq!(tested["ok"], false);

    let (s, saved) = call(&r, Method::POST, "/api/v1/notification", Some(hook)).await;
    assert_eq!(s, StatusCode::CREATED, "{saved}");

    // The first dispatch only records where history ends.
    assert_eq!(omp_app::dispatch_notifications(&r.app).await.unwrap(), 0);

    let meta = omp_metadata_movie();
    let metadata_id = r.app.db.movies().upsert_metadata(&meta).await.unwrap();
    let profile = r.app.db.quality_profiles().list().await.unwrap()[0].id;
    let movie_id = r
        .app
        .db
        .movies()
        .add(&NewMovie {
            metadata_id,
            monitored: true,
            minimum_availability: MovieStatus::Announced,
            quality_profile_id: profile,
            path: "/movies/The Matrix (1999)".into(),
            root_folder_path: None,
            tags: vec![],
        })
        .await
        .unwrap();
    let event = |event, message: &str| HistoryRecord {
        id: 0,
        movie_id,
        event,
        date: chrono::Utc::now(),
        source_title: "The.Matrix.1999.1080p.BluRay.x264-GRP".into(),
        quality: QualityModel::default(),
        languages: vec![],
        download_id: Some("abc".into()),
        data: [("message".to_owned(), message.to_owned())].into(),
    };
    for h in [
        event(HistoryEvent::Grabbed, ""),
        // Subscribed to neither: nothing is sent for an import.
        event(HistoryEvent::DownloadFolderImported, ""),
        event(HistoryEvent::DownloadFailed, "disk on fire"),
    ] {
        r.app.db.history().insert(&h).await.unwrap();
    }
    assert_eq!(omp_app::dispatch_notifications(&r.app).await.unwrap(), 2);

    let grab: Value = serde_json::from_str(&received.recv().await.unwrap()).unwrap();
    assert_eq!(grab["event"], "grab");
    assert_eq!(grab["movie"]["title"], "The Matrix");
    assert_eq!(grab["movie"]["tmdbId"], 603);
    let failed: Value = serde_json::from_str(&received.recv().await.unwrap()).unwrap();
    assert_eq!(failed["event"], "download-failed");
    assert!(failed["message"].as_str().unwrap().contains("disk on fire"));

    // Nothing is replayed.
    assert_eq!(omp_app::dispatch_notifications(&r.app).await.unwrap(), 0);
}

fn omp_metadata_movie() -> omp_core::movie::MovieMetadata {
    omp_core::movie::MovieMetadata {
        tmdb_id: 603,
        imdb_id: Some("tt0133093".into()),
        title: "The Matrix".into(),
        original_title: "The Matrix".into(),
        original_language: "en".into(),
        year: 1999,
        runtime: 136,
        status: MovieStatus::Released,
        ..Default::default()
    }
}
