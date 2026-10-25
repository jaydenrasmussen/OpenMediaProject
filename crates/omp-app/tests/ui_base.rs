//! The UI is served from a folder or the binary, under any URL base.

use std::{borrow::Cow, path::Path, sync::Arc};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use omp_app::{App, AppConfig, router};
use omp_db::Db;
use omp_services::{MediaProbe, NoDiskCheck, ProbeInfo};
use tower::ServiceExt;

struct NoProbe;

#[async_trait]
impl MediaProbe for NoProbe {
    async fn probe(&self, _: &Path) -> Option<ProbeInfo> {
        None
    }
}

const PAGE: &str = r#"<a href="/__omp_base__/activity/">Activity</a><script src="/__omp_base__/_next/app.js"></script>"#;
const JS: &str = r#"const BASE="/__omp_base__";fetch(BASE+"/api/v1/health")"#;

fn write_ui(dir: &Path) {
    std::fs::create_dir_all(dir.join("_next/static")).unwrap();
    std::fs::create_dir_all(dir.join("activity")).unwrap();
    std::fs::write(dir.join("index.html"), PAGE).unwrap();
    std::fs::write(dir.join("activity/index.html"), "activity page").unwrap();
    std::fs::write(dir.join("_next/app.js"), JS).unwrap();
    std::fs::write(dir.join("_next/static/hashed.js"), JS).unwrap();
    std::fs::write(dir.join("logo.png"), [0u8, 159, 146, 150]).unwrap();
}

fn embedded(path: &str) -> Option<Cow<'static, [u8]>> {
    match path {
        "index.html" => Some(Cow::Borrowed(b"embedded shell /__omp_base__/x")),
        _ => None,
    }
}

async fn rig(base: &str, use_embedded: bool) -> (Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    write_ui(&dir.path().join("ui"));
    let mut config = AppConfig::new(dir.path().join("data"));
    config.ui_dir = dir.path().join("ui");
    config.url_base = base.into();
    if use_embedded {
        config.embedded_ui = Some(embedded);
    }
    let app = App::new(
        config,
        Db::in_memory().await.unwrap(),
        None,
        Arc::new(NoProbe),
        Arc::new(NoDiskCheck),
    )
    .await
    .unwrap();
    (router(app), dir)
}

async fn fetch(r: &Router, path: &str) -> (StatusCode, String, Vec<u8>) {
    let res = r
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let cache = res
        .headers()
        .get(header::CACHE_CONTROL)
        .map(|v| v.to_str().unwrap().to_owned())
        .unwrap_or_default();
    let body = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, cache, body)
}

fn text(b: Vec<u8>) -> String {
    String::from_utf8(b).unwrap()
}

#[tokio::test]
async fn without_a_base_the_placeholder_becomes_the_root() {
    let (r, _dir) = rig("", false).await;
    let (s, _, body) = fetch(&r, "/").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        text(body),
        r#"<a href="/activity/">Activity</a><script src="/_next/app.js"></script>"#
    );
    let (_, _, js) = fetch(&r, "/_next/app.js").await;
    assert_eq!(text(js), r#"const BASE="";fetch(BASE+"/api/v1/health")"#);
    // Pages by folder, with or without the slash; unknown pages get the shell.
    assert_eq!(text(fetch(&r, "/activity/").await.2), "activity page");
    assert_eq!(text(fetch(&r, "/activity").await.2), "activity page");
    assert!(text(fetch(&r, "/movie/12").await.2).contains("Activity"));
    // A missing file is missing, not the shell; binary files are untouched.
    assert_eq!(fetch(&r, "/_next/gone.js").await.0, StatusCode::NOT_FOUND);
    assert_eq!(fetch(&r, "/logo.png").await.2, [0u8, 159, 146, 150]);
    // Nothing escapes the folder.
    assert_ne!(fetch(&r, "/..%2F..%2Fetc%2Fpasswd").await.0, StatusCode::OK);
    // Caching: built files forever, pages never.
    assert_eq!(
        fetch(&r, "/_next/static/hashed.js").await.1,
        "public, max-age=31536000, immutable"
    );
    assert_eq!(fetch(&r, "/").await.1, "no-cache");
    assert_eq!(fetch(&r, "/api/v1/health").await.0, StatusCode::OK);
}

#[tokio::test]
async fn behind_a_prefix_everything_moves_under_it() {
    let (r, _dir) = rig("/media/", false).await;
    let (s, _, body) = fetch(&r, "/media/").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        text(body),
        r#"<a href="/media/activity/">Activity</a><script src="/media/_next/app.js"></script>"#
    );
    assert_eq!(
        text(fetch(&r, "/media/_next/app.js").await.2),
        r#"const BASE="/media";fetch(BASE+"/api/v1/health")"#
    );
    assert_eq!(text(fetch(&r, "/media/activity/").await.2), "activity page");
    assert_eq!(fetch(&r, "/media/api/v1/health").await.0, StatusCode::OK);
    // Without the prefix there is nothing, and the root points to it.
    assert_eq!(fetch(&r, "/api/v1/health").await.0, StatusCode::NOT_FOUND);
    let res = r
        .clone()
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(res.headers()[header::LOCATION], "/media/");
}

#[tokio::test]
async fn a_ui_built_into_the_binary_is_served_the_same_way() {
    let (r, _dir) = rig("/tv", true).await;
    assert_eq!(text(fetch(&r, "/tv/").await.2), "embedded shell /tv/x");
    // The folder is ignored when a UI is embedded.
    assert_eq!(
        text(fetch(&r, "/tv/activity/").await.2),
        "embedded shell /tv/x"
    );
    assert_eq!(fetch(&r, "/tv/_next/app.js").await.0, StatusCode::NOT_FOUND);
}
