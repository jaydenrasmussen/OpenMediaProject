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
    async fn search(
        &self,
        term: &str,
        year: Option<i32>,
    ) -> Result<Vec<MovieMetadata>, MetadataError> {
        if term.to_lowercase().contains("matrix") && year.is_none_or(|y| y == 1999) {
            Ok(vec![self.movie(603).await?])
        } else {
            Ok(vec![])
        }
    }
    async fn movie_by_imdb_id(&self, imdb: &str) -> Result<MovieMetadata, MetadataError> {
        if imdb == "tt0133093" {
            self.movie(603).await
        } else {
            Err(MetadataError::NotFound)
        }
    }
}

struct Rig {
    router: Router,
    key: String,
    dir: tempfile::TempDir,
}

async fn rig_with(tmdb: bool) -> Rig {
    build_rig(tmdb, false).await
}

/// `on_disk` uses a real database file, which backups need.
async fn build_rig(tmdb: bool, on_disk: bool) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let ui = dir.path().join("ui");
    std::fs::create_dir_all(ui.join("_next")).unwrap();
    std::fs::write(ui.join("index.html"), "<h1>shell</h1>").unwrap();
    std::fs::write(ui.join("_next/app.js"), "1").unwrap();
    let mut config = AppConfig::new(dir.path().join("data"));
    config.ui_dir = ui;
    let metadata = tmdb.then(|| Arc::new(FakeTmdb) as Arc<dyn MovieMetadataProvider>);
    let db = if on_disk {
        Db::open(&dir.path().join("data")).await.unwrap()
    } else {
        Db::in_memory().await.unwrap()
    };
    let app = App::new(
        config,
        db,
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

    let mut partial = created.clone();
    partial["items"].as_array_mut().unwrap().pop();
    let (s, e) = call(
        &r,
        Method::PUT,
        &format!("/api/v1/qualityprofile/{id}"),
        Some(partial),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::BAD_REQUEST,
        "profiles must cover every quality: {e}"
    );

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

#[tokio::test]
async fn lookup_finds_movies_and_marks_those_already_in_the_library() {
    let r = rig().await;
    let (s, v) = get(&r, "/api/v1/movie/lookup?term=the%20matrix%201999").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        (v[0]["title"].clone(), v[0]["in_library"].clone()),
        (json!("The Matrix"), json!(null))
    );
    assert_eq!(
        get(&r, "/api/v1/movie/lookup?term=the%20matrix%202003")
            .await
            .1,
        json!([]),
        "year narrows the search"
    );
    assert_eq!(
        get(&r, "/api/v1/movie/lookup?term=tmdb:603").await.1[0]["tmdb_id"],
        603
    );
    assert_eq!(
        get(&r, "/api/v1/movie/lookup?term=imdb:tt0133093").await.1[0]["tmdb_id"],
        603
    );
    assert_eq!(
        get(&r, "/api/v1/movie/lookup?term=tmdb:1").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&r, "/api/v1/movie/lookup?term=tmdb:abc").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get(&r, "/api/v1/movie/lookup?term=%20").await.0,
        StatusCode::BAD_REQUEST
    );

    let root = add_root(&r).await;
    let body = json!({ "tmdb_id": 603, "quality_profile_id": 1, "root_folder_path": root });
    assert_eq!(
        call(&r, Method::POST, "/api/v1/movie", Some(body)).await.0,
        StatusCode::CREATED
    );
    let (_, again) = get(&r, "/api/v1/movie/lookup?term=matrix").await;
    assert!(again[0]["in_library"].is_i64());

    let bare = rig_with(false).await;
    assert_eq!(
        get(&bare, "/api/v1/movie/lookup?term=matrix").await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn interactive_search_and_grab_validate_their_input() {
    let r = rig().await;
    assert_eq!(
        get(&r, "/api/v1/release?movie_id=999").await.0,
        StatusCode::NOT_FOUND
    );

    let root = add_root(&r).await;
    let body = json!({ "tmdb_id": 603, "quality_profile_id": 1, "root_folder_path": root });
    let (_, movie) = call(&r, Method::POST, "/api/v1/movie", Some(body)).await;
    let (s, found) = get(&r, &format!("/api/v1/release?movie_id={}", movie["id"])).await;
    assert_eq!(s, StatusCode::OK, "{found}");
    assert_eq!(found["releases"], json!([]), "no indexers configured");

    for (id, status) in [
        ("garbage", StatusCode::BAD_REQUEST),
        ("99-0", StatusCode::NOT_FOUND),
    ] {
        let (s, _) = call(
            &r,
            Method::POST,
            "/api/v1/release",
            Some(json!({ "id": id })),
        )
        .await;
        assert_eq!(s, status, "{id}");
    }
}

#[tokio::test]
async fn provider_settings_can_be_tested_before_saving() {
    let r = rig().await;
    let dir = r.dir.path().join("torrents");
    let torrent =
        json!({ "name": "t", "implementation": "torrent", "settings": { "download_dir": dir } });
    let (s, v) = call(
        &r,
        Method::POST,
        "/api/v1/downloadclient/test",
        Some(torrent),
    )
    .await;
    assert_eq!((s, v["ok"].clone()), (StatusCode::OK, json!(true)), "{v}");
    assert!(dir.exists(), "the folder was created to prove it can be");

    let usenet = json!({ "name": "u", "implementation": "usenet", "settings": { "servers": [{ "host": "127.0.0.1", "port": 1, "tls": false }] } });
    let (_, v) = call(
        &r,
        Method::POST,
        "/api/v1/downloadclient/test",
        Some(usenet),
    )
    .await;
    assert_eq!(v["ok"], false);
    assert!(v["error"].as_str().unwrap().contains("127.0.0.1"), "{v}");
    let none = json!({ "name": "u", "implementation": "usenet", "settings": { "servers": [] } });
    assert_eq!(
        call(&r, Method::POST, "/api/v1/downloadclient/test", Some(none))
            .await
            .1["ok"],
        false
    );

    let indexer = json!({ "name": "i", "implementation": "newznab", "settings": { "base_url": "http://127.0.0.1:1", "api_key": "k" } });
    let (s, v) = call(&r, Method::POST, "/api/v1/indexer/test", Some(indexer)).await;
    assert_eq!((s, v["ok"].clone()), (StatusCode::OK, json!(false)), "{v}");
    let bad = json!({ "name": "i", "implementation": "gopher" });
    assert_eq!(
        call(&r, Method::POST, "/api/v1/indexer/test", Some(bad))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get(&r, "/api/v1/indexer").await.1,
        json!([]),
        "testing saves nothing"
    );
}

#[tokio::test]
async fn health_checks_report_what_is_missing_and_clear_as_it_is_fixed() {
    let r = rig().await;
    let sources = |v: &Value, level: &str| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .filter(|i| i["level"] == level)
            .map(|i| i["source"].as_str().unwrap().to_owned())
            .collect()
    };
    let (s, v) = get(&r, "/api/v1/system/health").await;
    assert_eq!(s, StatusCode::OK);
    let errors = sources(&v, "error");
    for expected in ["RootFolders", "Indexers", "DownloadClients"] {
        assert!(
            errors.contains(&expected.to_owned()),
            "{expected} missing from {v}"
        );
    }
    assert!(
        !errors.contains(&"Metadata".to_owned()),
        "TMDB is configured in this rig"
    );
    assert_eq!(v[0]["level"], "error", "errors come first");

    add_root(&r).await;
    let indexer = json!({ "name": "i", "implementation": "newznab", "settings": { "base_url": "http://127.0.0.1:1", "api_key": "k" } });
    call(&r, Method::POST, "/api/v1/indexer", Some(indexer)).await;
    let (_, v) = get(&r, "/api/v1/system/health").await;
    let errors = sources(&v, "error");
    assert!(
        !errors.contains(&"RootFolders".to_owned()) && !errors.contains(&"Indexers".to_owned()),
        "{v}"
    );
    assert!(errors.contains(&"DownloadClients".to_owned()));

    // A client that is configured but cannot start is called out by name.
    let broken =
        json!({ "name": "Broken", "implementation": "usenet", "settings": { "servers": [] } });
    call(&r, Method::POST, "/api/v1/downloadclient", Some(broken)).await;
    let (_, v) = get(&r, "/api/v1/system/health").await;
    assert!(
        v.as_array()
            .unwrap()
            .iter()
            .any(|i| i["message"].as_str().unwrap().contains("Broken")),
        "{v}"
    );

    let bare = rig_with(false).await;
    let (_, v) = get(&bare, "/api/v1/system/health").await;
    assert!(sources(&v, "error").contains(&"Metadata".to_owned()));
}

#[tokio::test]
async fn backups_can_be_made_listed_downloaded_and_deleted() {
    let r = build_rig(true, true).await;
    assert_eq!(get(&r, "/api/v1/system/backup").await.1, json!([]));
    let (s, made) = call(&r, Method::POST, "/api/v1/system/backup", None).await;
    assert_eq!(s, StatusCode::CREATED, "{made}");
    let name = made["name"].as_str().unwrap().to_owned();
    assert!(name.starts_with("omp_backup_") && name.ends_with(".db"));
    // Two in the same second do not clash.
    let (_, second) = call(&r, Method::POST, "/api/v1/system/backup", None).await;
    assert_ne!(second["name"], made["name"]);
    assert_eq!(
        get(&r, "/api/v1/system/backup")
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let res = r
        .router
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/system/backup/{name}"))
                .header("x-api-key", &r.key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    assert!(
        bytes.starts_with(b"SQLite format 3"),
        "it is a database file"
    );

    // Only names we made are reachable.
    for bad in [
        "..%2F..%2Fetc%2Fpasswd",
        "omp.db",
        "omp_backup_x.db",
        "omp_backup_.db",
    ] {
        assert_eq!(
            get(&r, &format!("/api/v1/system/backup/{bad}")).await.0,
            StatusCode::NOT_FOUND,
            "{bad}"
        );
        assert_eq!(
            call(
                &r,
                Method::DELETE,
                &format!("/api/v1/system/backup/{bad}"),
                None
            )
            .await
            .0,
            StatusCode::NOT_FOUND,
            "{bad}"
        );
    }
    assert_eq!(
        call(
            &r,
            Method::DELETE,
            &format!("/api/v1/system/backup/{name}"),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(&r, "/api/v1/system/backup")
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn custom_formats_are_validated_and_managed() {
    let r = rig().await;
    assert_eq!(get(&r, "/api/v1/customformat").await.1, json!([]));
    let good = json!({
        "name": "x265",
        "include_when_renaming": true,
        "specifications": [
            { "name": "x265", "negate": false, "required": true, "implementation": "release_title", "value": "[xh]\\.?265|hevc" }
        ]
    });
    let (s, created) = call(&r, Method::POST, "/api/v1/customformat", Some(good.clone())).await;
    assert_eq!(s, StatusCode::CREATED, "{created}");
    let id = created["id"].as_i64().unwrap();
    assert!(id > 0);

    let mut bad_regex = good.clone();
    bad_regex["specifications"][0]["value"] = json!("(unclosed");
    let (s, e) = call(&r, Method::POST, "/api/v1/customformat", Some(bad_regex)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{e}");
    let mut nameless = good.clone();
    nameless["name"] = json!(" ");
    assert_eq!(
        call(&r, Method::POST, "/api/v1/customformat", Some(nameless))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );

    let mut renamed = created.clone();
    renamed["name"] = json!("HEVC");
    assert_eq!(
        call(
            &r,
            Method::PUT,
            &format!("/api/v1/customformat/{id}"),
            Some(renamed)
        )
        .await
        .1["name"],
        "HEVC"
    );
    assert_eq!(get(&r, "/api/v1/customformat").await.1[0]["name"], "HEVC");
    assert_eq!(
        call(
            &r,
            Method::DELETE,
            &format!("/api/v1/customformat/{id}"),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(get(&r, "/api/v1/customformat").await.1, json!([]));
}

#[tokio::test]
async fn delay_profiles_release_profiles_and_tags() {
    let r = rig().await;

    let (_, delays) = get(&r, "/api/v1/delayprofile").await;
    assert_eq!(delays.as_array().unwrap().len(), 1, "the catch-all exists");
    let default_id = delays[0]["id"].as_i64().unwrap();
    let (s, e) = call(
        &r,
        Method::DELETE,
        &format!("/api/v1/delayprofile/{default_id}"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT, "the default cannot go: {e}");

    let (_, tag) = call(
        &r,
        Method::POST,
        "/api/v1/tag",
        Some(json!({ "label": "kids" })),
    )
    .await;
    let tag_id = tag["id"].as_i64().unwrap();
    assert_eq!(
        call(
            &r,
            Method::POST,
            "/api/v1/tag",
            Some(json!({ "label": " " }))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let mut custom = delays[0].clone();
    custom["tags"] = json!([tag_id]);
    custom["usenet_delay"] = json!(120);
    custom["order"] = json!(1);
    let (s, made) = call(&r, Method::POST, "/api/v1/delayprofile", Some(custom)).await;
    assert_eq!(s, StatusCode::CREATED, "{made}");
    assert_eq!(
        get(&r, "/api/v1/delayprofile")
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let made_id = made["id"].as_i64().unwrap();
    assert_eq!(
        call(
            &r,
            Method::DELETE,
            &format!("/api/v1/delayprofile/{made_id}"),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    let rp = json!({ "name": "no cam", "enabled": true, "required": [], "ignored": ["CAM", "TS"], "indexer_id": 0, "tags": [] });
    let (s, created) = call(&r, Method::POST, "/api/v1/releaseprofile", Some(rp)).await;
    assert_eq!(s, StatusCode::CREATED, "{created}");
    let rid = created["id"].as_i64().unwrap();
    let mut off = created.clone();
    off["enabled"] = json!(false);
    assert_eq!(
        call(
            &r,
            Method::PUT,
            &format!("/api/v1/releaseprofile/{rid}"),
            Some(off)
        )
        .await
        .1["enabled"],
        false
    );
    assert_eq!(
        call(
            &r,
            Method::DELETE,
            &format!("/api/v1/releaseprofile/{rid}"),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    assert_eq!(get(&r, "/api/v1/tag").await.1.as_array().unwrap().len(), 1);
    assert_eq!(
        call(&r, Method::DELETE, &format!("/api/v1/tag/{tag_id}"), None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn quality_definitions_and_decision_settings() {
    let r = rig().await;
    let (_, defs) = get(&r, "/api/v1/qualitydefinition").await;
    assert_eq!(
        defs.as_array().unwrap().len(),
        30,
        "every quality has a size definition"
    );
    let hd = defs
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["title"] == "Bluray-1080p")
        .unwrap()
        .clone();

    let (s, v) = call(
        &r,
        Method::PUT,
        &format!("/api/v1/qualitydefinition/{}", hd["quality_id"]),
        Some(json!({ "min_size": 5.0, "max_size": 200.0, "preferred_size": 150.0 })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, again) = get(&r, "/api/v1/qualitydefinition").await;
    let hd2 = again
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["title"] == "Bluray-1080p")
        .unwrap();
    assert_eq!(
        (
            hd2["min_size"].clone(),
            hd2["max_size"].clone(),
            hd2["preferred_size"].clone()
        ),
        (json!(5.0), json!(200.0), json!(150.0))
    );

    let id = hd["quality_id"].clone();
    for bad in [
        json!({ "min_size": -1.0 }),
        json!({ "min_size": 50.0, "max_size": 10.0 }),
        json!({ "max_size": 10.0, "preferred_size": 20.0 }),
    ] {
        assert_eq!(
            call(
                &r,
                Method::PUT,
                &format!("/api/v1/qualitydefinition/{id}"),
                Some(bad)
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        call(
            &r,
            Method::PUT,
            "/api/v1/qualitydefinition/9999",
            Some(json!({}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    let (_, mut cfg) = get(&r, "/api/v1/config/decision").await;
    cfg["minimum_age"] = json!(90);
    cfg["retention"] = json!(1500);
    let (s, saved) = call(&r, Method::PUT, "/api/v1/config/decision", Some(cfg)).await;
    assert_eq!(s, StatusCode::OK, "{saved}");
    let (_, back) = get(&r, "/api/v1/config/decision").await;
    assert_eq!(
        (back["minimum_age"].clone(), back["retention"].clone()),
        (json!(90), json!(1500))
    );
}

#[tokio::test]
async fn an_existing_library_can_be_found_and_imported() {
    let r = rig().await;
    let root = add_root(&r).await;
    let matrix = std::path::Path::new(&root).join("The Matrix (1999)");
    std::fs::create_dir_all(&matrix).unwrap();
    std::fs::write(
        matrix.join("The.Matrix.1999.1080p.BluRay.x264-GRP.mkv"),
        vec![1u8; 2048],
    )
    .unwrap();
    std::fs::create_dir_all(std::path::Path::new(&root).join("Mystery Folder")).unwrap();

    let (_, roots) = get(&r, "/api/v1/rootfolder").await;
    let root_id = roots[0]["id"].as_i64().unwrap();
    let (s, found) = get(&r, &format!("/api/v1/rootfolder/{root_id}/unmapped")).await;
    assert_eq!(s, StatusCode::OK, "{found}");
    let names: Vec<_> = found
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["Mystery Folder", "The Matrix (1999)"]);
    let m = found
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["title"] == "The Matrix")
        .unwrap();
    assert_eq!(
        (m["year"].clone(), m["has_media"].clone()),
        (json!(1999), json!(true))
    );
    assert_eq!(
        get(&r, "/api/v1/rootfolder/999/unmapped").await.0,
        StatusCode::NOT_FOUND
    );

    let (_, profiles) = get(&r, "/api/v1/qualityprofile").await;
    let profile = profiles[0]["id"].clone();
    let batch = |items: Value| json!({ "items": items, "quality_profile_id": profile });
    let path = matrix.to_string_lossy().into_owned();

    let (s, out) = call(
        &r,
        Method::POST,
        "/api/v1/movie/import",
        Some(batch(json!([
            { "path": path, "tmdb_id": 603 },
            { "path": "/etc", "tmdb_id": 603 },
            { "path": format!("{root}/Mystery Folder"), "tmdb_id": 999 },
        ]))),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{out}");
    let results = out["results"].as_array().unwrap();
    assert_eq!(results[0]["ok"], true, "{out}");
    assert_eq!(
        results[1]["ok"], false,
        "outside the root folders is refused: {out}"
    );
    assert_eq!(results[2]["ok"], false, "TMDB does not know 999: {out}");

    // The movie uses its existing folder and already has its file.
    let id = results[0]["movie_id"].as_i64().unwrap();
    let (_, movie) = get(&r, &format!("/api/v1/movie/{id}")).await;
    assert_eq!(movie["path"], path);
    assert_eq!(movie["has_file"], true);
    assert_eq!(movie["file"]["quality"], "Bluray-1080p");

    // Importing it again is refused, and it no longer shows up as unmapped.
    let (_, again) = call(
        &r,
        Method::POST,
        "/api/v1/movie/import",
        Some(batch(json!([{ "path": path, "tmdb_id": 603 }]))),
    )
    .await;
    assert_eq!(again["results"][0]["ok"], false);
    let (_, left) = get(&r, &format!("/api/v1/rootfolder/{root_id}/unmapped")).await;
    assert_eq!(left.as_array().unwrap().len(), 1);

    // A rescan notices the file is gone.
    std::fs::remove_file(matrix.join("The.Matrix.1999.1080p.BluRay.x264-GRP.mkv")).unwrap();
    let (s, _) = call(
        &r,
        Method::POST,
        "/api/v1/command",
        Some(json!({ "name": "RescanMovie", "movie_id": id, "wait": true })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        get(&r, &format!("/api/v1/movie/{id}")).await.1["has_file"],
        false
    );

    let bare = rig_with(false).await;
    assert_eq!(
        call(
            &bare,
            Method::POST,
            "/api/v1/movie/import",
            Some(batch(json!([])))
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
}
