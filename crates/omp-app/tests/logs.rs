use std::{path::Path, sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use omp_app::{App, AppConfig, log_channel, router, spawn_scheduler};
use omp_db::Db;
use omp_services::{MediaProbe, NoDiskCheck, ProbeInfo};
use serde_json::Value;
use tower::ServiceExt;
use tracing_subscriber::layer::SubscriberExt;

struct NoProbe;

#[async_trait]
impl MediaProbe for NoProbe {
    async fn probe(&self, _: &Path) -> Option<ProbeInfo> {
        None
    }
}

async fn app() -> (Arc<App>, tempfile::TempDir) {
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
    (app, dir)
}

async fn get(app: &Arc<App>, method: Method, path: &str) -> (StatusCode, Value) {
    let key = app.api_key().await.unwrap();
    let res = router(app.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("x-api-key", key)
                .body(Body::empty())
                .unwrap(),
        )
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
async fn events_reach_the_log_with_noise_filtered_out() {
    let (app, _dir) = app().await;
    let (layer, queue) = log_channel();
    let writer = queue.spawn_writer(app.db.clone());

    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(target: "omp_app::tasks", "imported a movie");
        tracing::warn!(target: "omp_app::tasks", movie = "The Matrix", "something odd");
        tracing::debug!(target: "omp_app::tasks", "too chatty");
        tracing::info!(target: "hyper::proto", "connection details");
        tracing::error!(target: "reqwest::connect", "dns failed");
    });
    // Let the writer drain.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let (s, all) = get(&app, Method::GET, "/api/v1/log").await;
    assert_eq!(s, StatusCode::OK);
    let messages: Vec<_> = all
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["message"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        messages,
        [
            "dns failed",
            "something odd movie=The Matrix",
            "imported a movie"
        ],
        "newest first; debug and other crates' info are left out"
    );

    let (_, warnings) = get(&app, Method::GET, "/api/v1/log?level=warn").await;
    assert_eq!(warnings.as_array().unwrap().len(), 2);
    let (_, found) = get(&app, Method::GET, "/api/v1/log?search=matrix").await;
    assert_eq!(found.as_array().unwrap().len(), 1);
    assert_eq!(found[0]["level"], "warn");
    assert_eq!(
        get(&app, Method::GET, "/api/v1/log?level=loud").await.0,
        StatusCode::BAD_REQUEST
    );

    assert_eq!(
        get(&app, Method::DELETE, "/api/v1/log").await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(&app, Method::GET, "/api/v1/log").await.1,
        serde_json::json!([])
    );
    writer.abort();
}

#[tokio::test]
async fn scheduled_tasks_are_listed_with_their_schedule() {
    let (app, _dir) = app().await;
    let jobs = spawn_scheduler(app.clone());
    let (s, tasks) = get(&app, Method::GET, "/api/v1/system/task").await;
    for j in jobs {
        j.abort();
    }
    assert_eq!(s, StatusCode::OK);
    let names: Vec<_> = tasks
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "backup",
            "import-lists",
            "notifications",
            "process-downloads",
            "recycle-bin",
            "refresh-movies",
            "rss-sync"
        ]
    );
    let rss = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "rss-sync")
        .unwrap();
    assert_eq!(rss["interval_secs"], 900);
}
