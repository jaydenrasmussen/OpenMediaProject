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
use omp_core::tv::{SeriesInfo, SeriesStatus};
use omp_core::{
    history::HistoryEvent,
    movie::{MovieMetadata, MovieStatus},
    quality::QualityModel,
};
use omp_db::{
    Db,
    repo::{HistoryRecord, NewMovie},
};
use omp_metadata::{MetadataError, MovieMetadataProvider, SeriesMetadataProvider};
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

/// Serves `body` to every request.
async fn serve_json(body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/list.json", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    url
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
        install_plugin(&plugin_dir, "webhook", "webhook.wasm");
        install_plugin(&plugin_dir, "toy-indexer", "toy_indexer.wasm");
        install_plugin(&plugin_dir, "custom-list", "custom_list.wasm");
    }
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

#[tokio::test]
async fn plugins_are_listed_and_describe_their_settings() {
    let r = rig(true).await;
    let (s, list) = call(&r, Method::GET, "/api/v1/plugin", None).await;
    assert_eq!(s, StatusCode::OK);
    let kinds: Vec<_> = list["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["id"].as_str().unwrap(), p["kind"].as_str().unwrap()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("custom-list", "list"),
            ("toy-indexer", "indexer"),
            ("webhook", "notification")
        ]
    );
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

async fn add_matrix(r: &Rig) -> i64 {
    let metadata_id = r
        .app
        .db
        .movies()
        .upsert_metadata(&omp_metadata_movie())
        .await
        .unwrap();
    let profile = r.app.db.quality_profiles().list().await.unwrap()[0].id;
    r.app
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
        .unwrap()
}

#[tokio::test]
async fn indexer_plugins_are_validated_tested_and_searched() {
    let r = rig(true).await;
    let toy = |settings: Value| json!({ "name": "Toy", "implementation": "toy-indexer", "plugin_id": "toy-indexer", "settings": settings });

    assert_eq!(
        call(
            &r,
            Method::POST,
            "/api/v1/indexer",
            Some(json!({ "name": "x", "implementation": "ghost", "plugin_id": "ghost", "settings": {} }))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, ok) = call(
        &r,
        Method::POST,
        "/api/v1/indexer/test",
        Some(toy(json!({}))),
    )
    .await;
    assert_eq!(ok["ok"], true, "{ok}");
    let (_, bad) = call(
        &r,
        Method::POST,
        "/api/v1/indexer/test",
        Some(toy(json!({ "broken": true }))),
    )
    .await;
    assert_eq!(
        (bad["ok"].clone(), bad["error"].clone()),
        (json!(false), json!("toy plugin is broken"))
    );

    let (s, saved) = call(&r, Method::POST, "/api/v1/indexer", Some(toy(json!({})))).await;
    assert_eq!(s, StatusCode::CREATED, "{saved}");

    // Interactive search asks the plugin and lists what it found.
    let movie_id = add_matrix(&r).await;
    let (s, found) = call(
        &r,
        Method::GET,
        &format!("/api/v1/release?movie_id={movie_id}"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{found}");
    assert!(
        found["indexer_failures"].as_array().unwrap().is_empty(),
        "{found}"
    );
    let releases = found["releases"].as_array().unwrap();
    assert_eq!(releases.len(), 1, "{found}");
    assert_eq!(releases[0]["title"], "The Matrix 1080p WEB-DL");
    assert_eq!(releases[0]["indexer"], "Toy");
}

#[tokio::test]
async fn import_lists_add_movies_once_and_respect_exclusions() {
    let r = rig(true).await;
    let url = serve_json(
        r#"[{"tmdbId": 603, "title": "The Matrix", "year": 1999},
            {"tmdb_id": "604", "title": "The Matrix Reloaded"},
            {"title": "no id, ignored"}]"#,
    )
    .await;

    let root = r._dir.path().join("movies");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.to_string_lossy().into_owned();
    assert_eq!(
        call(
            &r,
            Method::POST,
            "/api/v1/rootfolder",
            Some(json!({ "path": root }))
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let profile = r.app.db.quality_profiles().list().await.unwrap()[0].id;
    let list = |root: &str, profile: i32| {
        json!({
            "name": "Mirror", "implementation": "custom-list", "plugin_id": "custom-list",
            "settings": {
                "quality_profile_id": profile, "root_folder_path": root,
                "settings": { "url": url },
            },
        })
    };

    // The destination has to exist.
    for bad in [list("/nowhere", profile), list(&root, 9999)] {
        assert_eq!(
            call(&r, Method::POST, "/api/v1/importlist", Some(bad))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    let (_, tested) = call(
        &r,
        Method::POST,
        "/api/v1/importlist/test",
        Some(list(&root, profile)),
    )
    .await;
    assert_eq!(tested["ok"], true, "{tested}");
    let (s, saved) = call(
        &r,
        Method::POST,
        "/api/v1/importlist",
        Some(list(&root, profile)),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{saved}");

    let sync = || {
        call(
            &r,
            Method::POST,
            "/api/v1/command",
            Some(json!({ "name": "ImportListSync", "wait": true })),
        )
    };
    assert_eq!(sync().await.0, StatusCode::OK);
    let titles = |movies: Value| -> Vec<String> {
        let mut t: Vec<_> = movies
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["title"].as_str().unwrap().to_owned())
            .collect();
        t.sort();
        t
    };
    let (_, movies) = call(&r, Method::GET, "/api/v1/movie", None).await;
    assert_eq!(
        titles(movies.clone()),
        ["The Matrix", "The Matrix Reloaded"]
    );

    // Syncing again changes nothing.
    assert_eq!(sync().await.0, StatusCode::OK);
    let (_, again) = call(&r, Method::GET, "/api/v1/movie", None).await;
    assert_eq!(again.as_array().unwrap().len(), 2);

    // Removing a movie "and excluding it" keeps the list from adding it back.
    let reloaded = movies
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["tmdb_id"] == 604)
        .unwrap();
    let id = reloaded["id"].as_i64().unwrap();
    assert_eq!(
        call(
            &r,
            Method::DELETE,
            &format!("/api/v1/movie/{id}?add_exclusion=true"),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(sync().await.0, StatusCode::OK);
    let (_, left) = call(&r, Method::GET, "/api/v1/movie", None).await;
    assert_eq!(titles(left), ["The Matrix"]);
    let (_, exclusions) = call(&r, Method::GET, "/api/v1/exclusion", None).await;
    assert_eq!(exclusions[0]["tmdb_id"], 604);

    // Lifting the exclusion lets the next sync add it again.
    assert_eq!(
        call(&r, Method::DELETE, "/api/v1/exclusion/604", None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&r, Method::DELETE, "/api/v1/exclusion/604", None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(sync().await.0, StatusCode::OK);
    let (_, back) = call(&r, Method::GET, "/api/v1/movie", None).await;
    assert_eq!(back.as_array().unwrap().len(), 2);
}

struct FakeSeries;

#[async_trait]
impl SeriesMetadataProvider for FakeSeries {
    async fn series(&self, tmdb_id: i64) -> Result<SeriesInfo, MetadataError> {
        if tmdb_id == 1396 {
            Ok(SeriesInfo {
                tmdb_id: 1396,
                tvdb_id: Some(81189),
                title: "Breaking Bad".into(),
                year: 2008,
                status: SeriesStatus::Ended,
                ..Default::default()
            })
        } else {
            Err(MetadataError::NotFound)
        }
    }
    async fn search_series(
        &self,
        _: &str,
        _: Option<i32>,
    ) -> Result<Vec<SeriesInfo>, MetadataError> {
        Ok(vec![])
    }
    async fn series_by_imdb_id(&self, _: &str) -> Result<SeriesInfo, MetadataError> {
        Err(MetadataError::NotFound)
    }
    async fn series_by_tvdb_id(&self, tvdb: i64) -> Result<SeriesInfo, MetadataError> {
        if tvdb == 81189 {
            self.series(1396).await
        } else {
            Err(MetadataError::NotFound)
        }
    }
}

#[tokio::test]
async fn import_lists_add_series_named_by_tvdb_id() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = AppConfig::new(dir.path().join("data"));
    let plugin_dir = dir.path().join("plugins");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    install_plugin(&plugin_dir, "custom-list", "custom_list.wasm");
    config.plugin_dirs = vec![plugin_dir];
    let app = App::with_series_metadata(
        config,
        Db::in_memory().await.unwrap(),
        Some(Arc::new(FakeTmdb)),
        Some(Arc::new(FakeSeries)),
        Arc::new(NoProbe),
        Arc::new(NoDiskCheck),
    )
    .await
    .unwrap();
    let key = app.api_key().await.unwrap();
    let r = Rig {
        router: router(app.clone()),
        app,
        key,
        _dir: dir,
    };

    let url = serve_json(
        r#"[{"type": "series", "tvdbId": 81189, "title": "Breaking Bad"},
            {"tmdbId": 603, "title": "The Matrix"}]"#,
    )
    .await;
    let movies = r._dir.path().join("movies");
    let shows = r._dir.path().join("tv");
    for p in [&movies, &shows] {
        std::fs::create_dir_all(p).unwrap();
        call(
            &r,
            Method::POST,
            "/api/v1/rootfolder",
            Some(json!({ "path": p })),
        )
        .await;
    }
    let profile = r.app.db.quality_profiles().list().await.unwrap()[0].id;
    let body = |series_root: Option<&str>| {
        let mut settings = json!({
            "quality_profile_id": profile, "root_folder_path": movies,
            "settings": { "url": url },
        });
        if let Some(root) = series_root {
            settings["series_root_folder_path"] = json!(root);
            settings["series_quality_profile_id"] = json!(profile);
        }
        json!({ "name": "Mixed", "implementation": "custom-list", "plugin_id": "custom-list", "settings": settings })
    };
    assert_eq!(
        call(
            &r,
            Method::POST,
            "/api/v1/importlist",
            Some(body(Some("/nowhere")))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (s, _) = call(&r, Method::POST, "/api/v1/importlist", Some(body(None))).await;
    assert_eq!(s, StatusCode::CREATED);

    // Without a series destination the series are reported, the movie still arrives.
    let sync = || {
        call(
            &r,
            Method::POST,
            "/api/v1/command",
            Some(json!({ "name": "ImportListSync", "wait": true })),
        )
    };
    let (status, _) = sync().await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "the series problem is reported"
    );
    assert_eq!(
        call(&r, Method::GET, "/api/v1/movie", None)
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        call(&r, Method::GET, "/api/v1/series", None).await.1,
        json!([])
    );

    // With one, the series is added by its TVDB id, once.
    let (_, lists) = call(&r, Method::GET, "/api/v1/importlist", None).await;
    let id = lists[0]["id"].as_i64().unwrap();
    let (s, _) = call(
        &r,
        Method::PUT,
        &format!("/api/v1/importlist/{id}"),
        Some(body(Some(&shows.to_string_lossy()))),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(sync().await.0, StatusCode::OK);
    let (_, series) = call(&r, Method::GET, "/api/v1/series", None).await;
    assert_eq!(series.as_array().unwrap().len(), 1);
    assert_eq!(series[0]["title"], "Breaking Bad");
    assert_eq!(sync().await.0, StatusCode::OK);
    assert_eq!(
        call(&r, Method::GET, "/api/v1/series", None)
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
