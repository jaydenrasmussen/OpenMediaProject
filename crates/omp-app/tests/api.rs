use std::{path::Path, sync::Arc};

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
use tower::ServiceExt;

struct FakeProbe;

#[async_trait]
impl MediaProbe for FakeProbe {
    async fn probe(&self, _: &Path) -> Option<ProbeInfo> {
        None
    }
}

struct FakeTmdb;

#[async_trait]
impl MovieMetadataProvider for FakeTmdb {
    async fn movie(&self, tmdb_id: i64) -> Result<MovieMetadata, MetadataError> {
        if tmdb_id != 603 {
            return Err(MetadataError::NotFound);
        }
        Ok(MovieMetadata {
            tmdb_id: 603,
            imdb_id: Some("tt0133093".into()),
            title: "The Matrix".into(),
            original_title: "The Matrix".into(),
            original_language: "en".into(),
            year: 1999,
            runtime: 136,
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

struct Rig {
    router: Router,
    key: String,
    dir: tempfile::TempDir,
}

async fn rig_with(tmdb: bool) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let ui = dir.path().join("ui");
    std::fs::create_dir_all(ui.join("_next")).unwrap();
    std::fs::write(ui.join("index.html"), "<h1>shell</h1>").unwrap();
    std::fs::write(ui.join("_next/app.js"), "1").unwrap();
    let mut config = AppConfig::new(dir.path().join("data"));
    config.ui_dir = ui;
    let metadata = tmdb.then(|| Arc::new(FakeTmdb) as Arc<dyn MovieMetadataProvider>);
    let app = App::new(
        config,
        Db::in_memory().await.unwrap(),
        metadata,
        Arc::new(FakeProbe),
        Arc::new(NoDiskCheck),
    )
    .await
    .unwrap();
    let key = app.api_key().await.unwrap();
    Rig {
        router: router(app),
        key,
        dir,
    }
}

async fn rig() -> Rig {
    rig_with(true).await
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

async fn get(r: &Rig, path: &str) -> (StatusCode, Value) {
    call(r, Method::GET, path, None).await
}

async fn add_root(r: &Rig) -> String {
    let path = r.dir.path().join("movies");
    std::fs::create_dir_all(&path).unwrap();
    let path = path.to_string_lossy().into_owned();
    let (s, _) = call(
        r,
        Method::POST,
        "/api/v1/rootfolder",
        Some(json!({ "path": path })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    path
}

#[tokio::test]
async fn health_status_and_ui_fallback() {
    let r = rig().await;
    let (s, v) = get(&r, "/api/v1/health").await;
    assert_eq!((s, v["status"].as_str()), (StatusCode::OK, Some("ok")));
    let (_, v) = get(&r, "/api/v1/system/status").await;
    assert_eq!(v["tmdb_configured"], true);

    let page = r
        .router
        .clone()
        .oneshot(Request::get("/movies/1").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK, "unknown page -> UI shell");
    let missing = r
        .router
        .clone()
        .oneshot(
            Request::get("/_next/missing.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let (s, _) = get(&r, "/api/v1/nope").await;
    assert_eq!(
        s,
        StatusCode::NOT_FOUND,
        "unknown API paths are not the shell"
    );
}

#[tokio::test]
async fn first_run_defaults_are_seeded() {
    let r = rig().await;
    let (_, profiles) = get(&r, "/api/v1/qualityprofile").await;
    let names: Vec<_> = profiles
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_owned())
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        [
            "Any",
            "HD - 720p/1080p",
            "HD-1080p",
            "HD-720p",
            "SD",
            "Ultra-HD"
        ]
    );
    assert_eq!(get(&r, "/api/v1/movie").await.1, json!([]));
    assert_eq!(get(&r, "/api/v1/queue").await.1, json!([]));
    assert_eq!(get(&r, "/api/v1/history").await.1, json!([]));
}

#[tokio::test]
async fn movies_can_be_added_edited_and_removed() {
    let r = rig().await;
    let root = add_root(&r).await;
    let (_, profiles) = get(&r, "/api/v1/qualityprofile").await;
    let hd = profiles
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "HD-1080p")
        .unwrap()["id"]
        .clone();

    let add = |tmdb: i64, root: &str| json!({ "tmdb_id": tmdb, "quality_profile_id": hd, "root_folder_path": root });
    let bad_profile =
        json!({ "tmdb_id": 603, "quality_profile_id": 9999, "root_folder_path": root });
    assert_eq!(
        call(&r, Method::POST, "/api/v1/movie", Some(bad_profile))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (s, m) = call(&r, Method::POST, "/api/v1/movie", Some(add(603, &root))).await;
    assert_eq!(s, StatusCode::CREATED, "{m}");
    assert_eq!(m["title"], "The Matrix");
    assert_eq!(
        m["path"],
        format!("{root}/The Matrix (1999)"),
        "folder from the naming pattern"
    );
    assert_eq!(
        (
            m["monitored"].clone(),
            m["minimum_availability"].clone(),
            m["has_file"].clone()
        ),
        (json!(true), json!("announced"), json!(false))
    );
    let id = m["id"].as_i64().unwrap();

    assert_eq!(
        call(&r, Method::POST, "/api/v1/movie", Some(add(603, &root)))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(&r, Method::POST, "/api/v1/movie", Some(add(999, &root)))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &r,
            Method::POST,
            "/api/v1/movie",
            Some(add(603, "/not/a/root"))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get(&r, "/api/v1/movie").await.1.as_array().unwrap().len(),
        1
    );
    let (s, one) = get(&r, &format!("/api/v1/movie/{id}")).await;
    assert_eq!((s, one["tmdb_id"].clone()), (StatusCode::OK, json!(603)));

    let (s, updated) = call(
        &r,
        Method::PUT,
        &format!("/api/v1/movie/{id}"),
        Some(json!({ "monitored": false, "minimum_availability": "released" })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{updated}");
    assert_eq!(
        (
            updated["monitored"].clone(),
            updated["minimum_availability"].clone()
        ),
        (json!(false), json!("released"))
    );
    let (s, _) = call(
        &r,
        Method::PUT,
        &format!("/api/v1/movie/{id}"),
        Some(json!({ "minimum_availability": "someday" })),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    assert_eq!(
        call(&r, Method::DELETE, &format!("/api/v1/movie/{id}"), None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(&r, &format!("/api/v1/movie/{id}")).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn adding_a_movie_needs_a_metadata_provider() {
    let r = rig_with(false).await;
    let root = add_root(&r).await;
    let body = json!({ "tmdb_id": 603, "quality_profile_id": 1, "root_folder_path": root });
    let (s, v) = call(&r, Method::POST, "/api/v1/movie", Some(body)).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert!(v["error"].as_str().unwrap().contains("TMDB_READ_TOKEN"));
}

#[tokio::test]
async fn root_folders_must_exist() {
    let r = rig().await;
    assert_eq!(
        call(
            &r,
            Method::POST,
            "/api/v1/rootfolder",
            Some(json!({ "path": "relative/dir" }))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &r,
            Method::POST,
            "/api/v1/rootfolder",
            Some(json!({ "path": "/definitely/not/here" }))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let root = add_root(&r).await;
    assert_eq!(
        call(
            &r,
            Method::POST,
            "/api/v1/rootfolder",
            Some(json!({ "path": root }))
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "no duplicates"
    );
    let (_, list) = get(&r, "/api/v1/rootfolder").await;
    let id = list[0]["id"].as_i64().unwrap();
    assert_eq!(
        call(
            &r,
            Method::DELETE,
            &format!("/api/v1/rootfolder/{id}"),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn quality_profiles_round_trip() {
    let r = rig().await;
    let (_, list) = get(&r, "/api/v1/qualityprofile").await;
    let mut p = list[0].clone();
    p["name"] = json!("My profile");
    let (s, created) = call(&r, Method::POST, "/api/v1/qualityprofile", Some(p)).await;
    assert_eq!(s, StatusCode::CREATED, "{created}");
    let id = created["id"].as_i64().unwrap();
    assert!(id > 6);

    let mut edited = created.clone();
    edited["upgrade_allowed"] = json!(false);
    let (s, saved) = call(
        &r,
        Method::PUT,
        &format!("/api/v1/qualityprofile/{id}"),
        Some(edited),
    )
    .await;
    assert_eq!(
        (s, saved["upgrade_allowed"].clone()),
        (StatusCode::OK, json!(false))
    );
    assert_eq!(
        call(
            &r,
            Method::DELETE,
            &format!("/api/v1/qualityprofile/{id}"),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn indexers_and_download_clients_are_managed_through_the_api() {
    let r = rig().await;
    let indexer = json!({ "name": "My Indexer", "implementation": "newznab", "settings": { "base_url": "https://idx.example", "api_key": "k" } });
    let (s, created) = call(&r, Method::POST, "/api/v1/indexer", Some(indexer)).await;
    assert_eq!(s, StatusCode::CREATED, "{created}");
    assert_eq!(
        (created["enable_rss"].clone(), created["priority"].clone()),
        (json!(true), json!(25))
    );
    let id = created["id"].as_i64().unwrap();

    assert_eq!(
        call(
            &r,
            Method::POST,
            "/api/v1/indexer",
            Some(json!({ "name": "x", "implementation": "gopher" }))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &r,
            Method::POST,
            "/api/v1/indexer",
            Some(json!({ "name": " ", "implementation": "newznab" }))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    let mut edited = created.clone();
    edited["enabled"] = json!(false);
    assert_eq!(
        call(
            &r,
            Method::PUT,
            &format!("/api/v1/indexer/{id}"),
            Some(edited)
        )
        .await
        .1["enabled"],
        json!(false)
    );
    assert_eq!(
        get(&r, "/api/v1/indexer").await.1.as_array().unwrap().len(),
        1
    );
    assert_eq!(
        get(&r, "/api/v1/downloadclient").await.1,
        json!([]),
        "kinds are kept apart"
    );
    assert_eq!(
        call(
            &r,
            Method::PUT,
            &format!("/api/v1/downloadclient/{id}"),
            Some(json!({ "name": "n", "implementation": "usenet" }))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    let usenet = json!({ "name": "Usenet", "implementation": "usenet", "settings": { "servers": [{ "host": "127.0.0.1", "port": 1, "tls": false }] } });
    let (s, client) = call(&r, Method::POST, "/api/v1/downloadclient", Some(usenet)).await;
    assert_eq!(s, StatusCode::CREATED, "{client}");
    assert_eq!(
        get(&r, "/api/v1/downloadclient")
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let cid = client["id"].as_i64().unwrap();
    assert_eq!(
        call(
            &r,
            Method::DELETE,
            &format!("/api/v1/downloadclient/{cid}"),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&r, Method::DELETE, &format!("/api/v1/indexer/{id}"), None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn commands_validate_and_run() {
    let r = rig().await;
    assert_eq!(
        call(
            &r,
            Method::POST,
            "/api/v1/command",
            Some(json!({ "name": "Explode" }))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &r,
            Method::POST,
            "/api/v1/command",
            Some(json!({ "name": "MoviesSearch" }))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (s, v) = call(
        &r,
        Method::POST,
        "/api/v1/command",
        Some(json!({ "name": "RssSync", "wait": true })),
    )
    .await;
    assert_eq!(
        (s, v["status"].clone()),
        (StatusCode::OK, json!("completed")),
        "no indexers is not an error"
    );
    let (s, _) = call(
        &r,
        Method::POST,
        "/api/v1/command",
        Some(json!({ "name": "ProcessDownloads" })),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn import_settings_persist() {
    let r = rig().await;
    let (_, mut c) = get(&r, "/api/v1/config/import").await;
    assert_eq!(c["naming"]["rename_movies"], false);
    c["naming"]["rename_movies"] = json!(true);
    c["naming"]["colon_replacement"] = json!("dash");
    c["mode"] = json!("copy");
    let (s, saved) = call(&r, Method::PUT, "/api/v1/config/import", Some(c)).await;
    assert_eq!(s, StatusCode::OK, "{saved}");
    let (_, again) = get(&r, "/api/v1/config/import").await;
    assert_eq!(
        (
            again["naming"]["rename_movies"].clone(),
            again["naming"]["colon_replacement"].clone(),
            again["mode"].clone()
        ),
        (json!(true), json!("dash"), json!("copy"))
    );
}
