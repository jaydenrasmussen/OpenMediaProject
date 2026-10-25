//! A Jellyfin library read by the real plugin through the API: what the
//! movies list says is already on the server, and what a sync records.

use std::{path::PathBuf, process::Command, sync::Arc};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use omp_app::{App, AppConfig, router};
use omp_core::movie::{MovieMetadata, MovieStatus};
use omp_db::Db;
use omp_metadata::{MetadataError, MovieMetadataProvider};
use omp_services::{MediaProbe, NoDiskCheck, ProbeInfo};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tower::ServiceExt;

struct NoProbe;

#[async_trait]
impl MediaProbe for NoProbe {
    async fn probe(&self, _: &std::path::Path) -> Option<ProbeInfo> {
        None
    }
}

/// Knows two movies.
struct FakeTmdb;

#[async_trait]
impl MovieMetadataProvider for FakeTmdb {
    async fn movie(&self, tmdb_id: i64) -> Result<MovieMetadata, MetadataError> {
        let (title, year) = match tmdb_id {
            603 => ("The Matrix", 1999),
            604 => ("The Matrix Reloaded", 2003),
            _ => return Err(MetadataError::NotFound),
        };
        Ok(MovieMetadata {
            tmdb_id,
            title: title.into(),
            original_title: title.into(),
            original_language: "en".into(),
            year,
            runtime: 130,
            status: MovieStatus::Released,
            ..Default::default()
        })
    }
    async fn search(&self, _: &str, _: Option<i32>) -> Result<Vec<MovieMetadata>, MetadataError> {
        Ok(vec![])
    }
    async fn movie_by_imdb_id(&self, _: &str) -> Result<MovieMetadata, MetadataError> {
        Err(MetadataError::NotFound)
    }
}

/// Builds `plugins/<name>` and lays it out the way the app expects.
fn install_plugin(into: &std::path::Path, name: &str, artifact: &str) {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins")
        .join(name);
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
    let dest = into.join(name);
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::copy(source.join("plugin.toml"), dest.join("plugin.toml")).unwrap();
    std::fs::copy(
        source.join("target/wasm32-wasip2/release").join(artifact),
        dest.join(artifact),
    )
    .unwrap();
}

struct Rig {
    app: Arc<App>,
    router: Router,
    key: String,
    _dir: tempfile::TempDir,
}

async fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut config = AppConfig::new(dir.path().join("data"));
    let plugin_dir = dir.path().join("plugins");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    install_plugin(&plugin_dir, "jellyfin-library", "jellyfin_library.wasm");
    config.plugin_dirs = vec![plugin_dir];
    let app = App::new(
        config,
        Db::in_memory().await.unwrap(),
        Some(Arc::new(FakeTmdb)),
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

/// A Jellyfin that wants the right key and holds The Matrix, Heat and one series.
async fn fake_jellyfin() -> String {
    use axum::{extract::Query, http::HeaderMap, routing::get};
    let app = Router::new()
        .route(
            "/System/Info",
            get(|h: HeaderMap| async move {
                if h.get("x-emby-token").and_then(|v| v.to_str().ok()) != Some("KEY") {
                    return (StatusCode::UNAUTHORIZED, String::new());
                }
                (StatusCode::OK, r#"{"ServerName":"Den","Version":"10.10"}"#.into())
            }),
        )
        .route(
            "/Items",
            get(|h: HeaderMap, Query(q): Query<std::collections::HashMap<String, String>>| async move {
                if h.get("x-emby-token").and_then(|v| v.to_str().ok()) != Some("KEY") {
                    return (StatusCode::UNAUTHORIZED, String::new());
                }
                let all = [
                    r#"{"Id":"m1","Type":"Movie","Name":"The Matrix","ProductionYear":1999,"ProviderIds":{"Tmdb":"603"}}"#,
                    r#"{"Id":"m2","Type":"Movie","Name":"Heat","ProductionYear":1995,"ProviderIds":{"Tmdb":"949"}}"#,
                    r#"{"Id":"s1","Type":"Series","Name":"Breaking Bad","ProductionYear":2008,"ProviderIds":{"Tmdb":"1396"}}"#,
                    r#"{"Id":"e1","Type":"Episode","SeriesName":"Breaking Bad","SeriesId":"s1","ParentIndexNumber":1,"IndexNumber":1}"#,
                    r#"{"Id":"e2","Type":"Episode","SeriesName":"Breaking Bad","SeriesId":"s1","ParentIndexNumber":1,"IndexNumber":2}"#,
                ];
                let start: usize = q.get("StartIndex").and_then(|s| s.parse().ok()).unwrap_or(0);
                // Two at a time, so a sync has to follow the pages.
                let page: Vec<&str> = all.iter().skip(start).take(2).copied().collect();
                (
                    StatusCode::OK,
                    format!(r#"{{"Items":[{}],"TotalRecordCount":{}}}"#, page.join(","), all.len()),
                )
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

async fn add_matrix(r: &Rig) {
    let dir = r._dir.path().join("movies");
    std::fs::create_dir_all(&dir).unwrap();
    let (_, root) = call(
        r,
        Method::POST,
        "/api/v1/rootfolder",
        Some(json!({ "path": dir })),
    )
    .await;
    let (_, profiles) = call(r, Method::GET, "/api/v1/qualityprofile", None).await;
    let (s, added) = call(
        r,
        Method::POST,
        "/api/v1/movie",
        Some(json!({
            "tmdb_id": 603,
            "quality_profile_id": profiles[0]["id"],
            "root_folder_path": root["path"],
        })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{added}");
}

fn servers(movies: &Value, title: &str) -> Value {
    movies
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["title"] == title)
        .unwrap()["media_servers"]
        .clone()
}

#[tokio::test]
async fn a_jellyfin_library_shows_what_is_already_there() {
    let r = rig().await;
    add_matrix(&r).await;
    let jellyfin = fake_jellyfin().await;
    let body = |key: &str| {
        json!({
            "name": "Den",
            "implementation": "jellyfin-library",
            "plugin_id": "jellyfin-library",
            "settings": { "url": jellyfin, "api_key": key },
        })
    };

    // Nothing is configured yet, so nothing is claimed.
    let (_, movies) = call(&r, Method::GET, "/api/v1/movie", None).await;
    assert_eq!(servers(&movies, "The Matrix"), json!([]));

    let (_, tested) = call(
        &r,
        Method::POST,
        "/api/v1/mediaserver/test",
        Some(body("KEY")),
    )
    .await;
    assert_eq!(tested, json!({ "ok": true, "server": "Den 10.10" }));
    let (_, wrong) = call(
        &r,
        Method::POST,
        "/api/v1/mediaserver/test",
        Some(body("NOPE")),
    )
    .await;
    assert_eq!(wrong["ok"], false);
    assert!(
        wrong["error"]
            .as_str()
            .unwrap()
            .contains("refused the API key"),
        "{wrong}"
    );

    // A server must name an installed plugin.
    let mut bad = body("KEY");
    bad["plugin_id"] = json!("nope");
    assert_eq!(
        call(&r, Method::POST, "/api/v1/mediaserver", Some(bad))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );

    let (s, saved) = call(&r, Method::POST, "/api/v1/mediaserver", Some(body("KEY"))).await;
    assert_eq!(s, StatusCode::CREATED, "{saved}");
    let id = saved["id"].as_i64().unwrap();
    let sync = || {
        call(
            &r,
            Method::POST,
            "/api/v1/command",
            Some(json!({ "name": "MediaServerSync", "wait": true })),
        )
    };
    assert_eq!(sync().await.0, StatusCode::OK);

    let (_, movies) = call(&r, Method::GET, "/api/v1/movie", None).await;
    assert_eq!(servers(&movies, "The Matrix"), json!(["Den"]));
    let (_, one) = call(&r, Method::GET, "/api/v1/movie/1", None).await;
    assert_eq!(one["media_servers"], json!(["Den"]));

    // A movie a server holds is not missing.
    let (_, wanted) = call(&r, Method::GET, "/api/v1/wanted/missing", None).await;
    assert!(
        wanted
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["title"] != "The Matrix"),
        "{wanted}"
    );

    // What the sync read, page by page.
    let (_, status) = call(&r, Method::GET, "/api/v1/mediaserver/status", None).await;
    let s = &status[0];
    assert_eq!(
        (
            s["provider_id"].as_i64(),
            s["movies"].as_i64(),
            s["series"].as_i64(),
            s["episodes"].as_i64()
        ),
        (Some(id), Some(2), Some(1), Some(2))
    );
    assert_eq!(s["error"], Value::Null);

    // Heat is on the server and not here; The Matrix is both.
    let (_, untracked) = call(&r, Method::GET, "/api/v1/mediaserver/untracked", None).await;
    let titles = |v: &Value| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|m| m["title"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(titles(&untracked["movies"]), ["Heat"]);
    assert_eq!(untracked["movies"][0]["servers"], json!(["Den"]));
    assert_eq!(titles(&untracked["series"]), ["Breaking Bad"]);

    // A server that starts refusing keeps its last good copy and says why.
    let mut wrong_key = body("OLD");
    wrong_key["settings"]["api_key"] = json!("CHANGED");
    let (s, _) = call(
        &r,
        Method::PUT,
        &format!("/api/v1/mediaserver/{id}"),
        Some(wrong_key),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(!sync().await.0.is_success(), "the failure is reported");
    let (_, status) = call(&r, Method::GET, "/api/v1/mediaserver/status", None).await;
    assert!(
        status[0]["error"]
            .as_str()
            .unwrap()
            .contains("refused the API key"),
        "{status}"
    );
    assert_eq!(status[0]["movies"], 2, "the last good counts stay");
    let (_, movies) = call(&r, Method::GET, "/api/v1/movie", None).await;
    assert_eq!(
        servers(&movies, "The Matrix"),
        json!(["Den"]),
        "and so does what it held"
    );

    // A disabled server claims nothing; a removed one is gone.
    let mut off = body("KEY");
    off["enabled"] = json!(false);
    call(
        &r,
        Method::PUT,
        &format!("/api/v1/mediaserver/{id}"),
        Some(off),
    )
    .await;
    r.app.refresh_presence().await;
    let (_, movies) = call(&r, Method::GET, "/api/v1/movie", None).await;
    assert_eq!(servers(&movies, "The Matrix"), json!([]));
    let (_, wanted) = call(&r, Method::GET, "/api/v1/wanted/missing", None).await;
    assert_eq!(servers(&wanted, "The Matrix"), json!([]), "{wanted}");
    let (s, _) = call(
        &r,
        Method::DELETE,
        &format!("/api/v1/mediaserver/{id}"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, status) = call(&r, Method::GET, "/api/v1/mediaserver/status", None).await;
    assert_eq!(status, json!([]), "its rows go with it");
}
