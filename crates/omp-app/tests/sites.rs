//! Torrent sites read from definitions, end to end: the list of sites, adding
//! one, an interactive search through it, and the download behind a private
//! site's sign-in.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use omp_app::{App, AppConfig, router};
use omp_core::movie::{MovieMetadata, MovieStatus};
use omp_db::{Db, repo::NewMovie};
use omp_services::{MediaProbe, NoDiskCheck, ProbeInfo};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tower::ServiceExt;

struct NoProbe;

#[async_trait]
impl MediaProbe for NoProbe {
    async fn probe(&self, _: &std::path::Path) -> Option<ProbeInfo> {
        None
    }
}

const RESULTS: &str = r#"<table class="results">
<tr><td class="n"><a href="/t/1">The.Matrix.1999.1080p.BluRay.x264-GRP</a></td><td class="s">1.5 GB</td><td class="se">42</td></tr>
<tr><td class="n"><a href="/t/2">The.Matrix.Reloaded.2003.1080p.BluRay.x264-GRP</a></td><td class="s">2 GB</td><td class="se">9</td></tr>
</table>"#;

/// A tiny web server: the results page, a details page with a download button, the torrent.
async fn serve_site() -> String {
    serve_site_recording().await.0
}

async fn serve_site_recording() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let record = seen.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let record = record.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
                record.lock().unwrap().push(path.clone());
                let (ctype, body): (&str, Vec<u8>) = if path.starts_with("/browse") {
                    ("text/html", RESULTS.as_bytes().to_vec())
                } else if path.starts_with("/t/") {
                    (
                        "text/html",
                        br#"<a class="get" href="/file/1.torrent">Download</a>"#.to_vec(),
                    )
                } else if path.starts_with("/file/") {
                    ("application/x-bittorrent", b"d8:announce3:urle".to_vec())
                } else {
                    ("text/plain", b"not found".to_vec())
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            });
        }
    });
    (base, seen)
}

fn definition(base: &str) -> String {
    format!(
        r#"id: fakesite
name: Fake Site
description: A site for tests
language: en-US
type: semi-private
links: ["{base}/"]
caps:
  categorymappings:
    - {{id: 1, cat: Movies/HD, desc: Movies}}
  modes: {{search: [q], movie-search: [q]}}
settings:
  - {{name: flavour, type: text, label: Flavour}}
search:
  paths: [{{path: /browse}}]
  inputs: {{q: "{{{{ .Keywords }}}}"}}
  rows: {{selector: "table.results tr"}}
  fields:
    title: {{selector: td.n a}}
    details: {{selector: td.n a, attribute: href}}
    download: {{selector: td.n a, attribute: href}}
    size: {{selector: td.s}}
    seeders: {{selector: td.se}}
    category: {{text: 1}}
download:
  selectors:
    - {{selector: a.get, attribute: href}}
"#
    )
}

struct Rig {
    app: Arc<App>,
    router: Router,
    key: String,
    _dir: tempfile::TempDir,
}

async fn rig(base: &str) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let config = AppConfig::new(dir.path().join("data"));
    std::fs::create_dir_all(config.data_dir.join("indexers")).unwrap();
    std::fs::write(
        config.data_dir.join("indexers/fakesite.yml"),
        definition(base),
    )
    .unwrap();
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

async fn call(
    r: &Rig,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value, Vec<u8>) {
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
    let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        bytes,
    )
}

async fn add_matrix(r: &Rig) -> i64 {
    let metadata_id = r
        .app
        .db
        .movies()
        .upsert_metadata(&MovieMetadata {
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

fn site(extra: Value) -> Value {
    json!({ "name": "Fake", "implementation": "cardigann", "settings": { "definition": "fakesite", "config": extra } })
}

#[tokio::test]
async fn the_catalogue_lists_torrent_sites_and_usenet_indexers() {
    let base = serve_site().await;
    let r = rig(&base).await;
    let (s, v, _) = call(&r, Method::GET, "/api/v1/indexer/catalogue", None).await;
    assert_eq!(s, StatusCode::OK);
    let torrent = v["torrent"].as_array().unwrap();
    assert!(torrent.len() > 500);
    let fake = torrent
        .iter()
        .find(|t| t["id"] == "fakesite")
        .expect("the owner's own definition is listed");
    assert_eq!(
        (&fake["privacy"], &fake["movies"]),
        (&json!("semi-private"), &json!(true))
    );
    assert!(fake.get("fields").is_none(), "the list stays light");
    assert!(
        torrent
            .iter()
            .any(|t| t["id"] == "thepiratebay" && t["privacy"] == "public")
    );
    assert!(
        v["usenet"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["name"] == "NZBgeek" && u["base_url"] == "https://api.nzbgeek.info")
    );

    let (s, detail, _) = call(&r, Method::GET, "/api/v1/indexer/site/fakesite", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(detail["fields"][0]["name"], "flavour");
    assert_eq!(
        call(&r, Method::GET, "/api/v1/indexer/site/nothere", None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_site_is_tested_saved_searched_and_its_torrent_fetched() {
    let base = serve_site().await;
    let r = rig(&base).await;

    let (_, bad, _) = call(&r, Method::POST, "/api/v1/indexer", Some(json!({"name": "x", "implementation": "cardigann", "settings": {"definition": "ghost"}}))).await;
    assert!(bad.to_string().contains("choose one of the sites"), "{bad}");

    let (_, tested, _) = call(
        &r,
        Method::POST,
        "/api/v1/indexer/test",
        Some(site(json!({"flavour": "x"}))),
    )
    .await;
    assert_eq!(tested["ok"], true, "{tested}");

    let (s, saved, _) = call(&r, Method::POST, "/api/v1/indexer", Some(site(json!({})))).await;
    assert_eq!(s, StatusCode::CREATED, "{saved}");
    let id = saved["id"].as_i64().unwrap();

    let movie_id = add_matrix(&r).await;
    let (s, found, _) = call(
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
    let matrix = releases
        .iter()
        .find(|x| x["title"] == "The.Matrix.1999.1080p.BluRay.x264-GRP")
        .unwrap_or_else(|| panic!("{found}"));
    assert_eq!(matrix["indexer"], "Fake");
    assert_eq!(matrix["seeders"], 42);
    assert_eq!(matrix["protocol"], "torrent");

    // A site that is not public is fetched through the app, which finds the file behind the page.
    let link = format!("{base}/t/1");
    let (s, _, bytes) = call(
        &r,
        Method::GET,
        &format!("/api/v1/indexer/{id}/download?link={link}"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(bytes, b"d8:announce3:urle");

    let (s, _, _) = call(
        &r,
        Method::GET,
        &format!("/api/v1/indexer/{id}/download?link=http://127.0.0.1:1/x"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn tv_searches_ask_the_site_for_the_episode_by_its_number() {
    use omp_core::tv::SeriesType;
    use omp_indexers::{TvSearch, TvSeries};
    let (base, seen) = serve_site_recording().await;
    let r = rig(&base).await;
    let (_, saved, _) = call(&r, Method::POST, "/api/v1/indexer", Some(site(json!({})))).await;
    let provider = r
        .app
        .db
        .providers()
        .get(saved["id"].as_i64().unwrap())
        .await
        .unwrap();
    let series = TvSeries {
        tvdb_id: Some(81189),
        imdb_id: None,
        tmdb_id: 1396,
        series_type: SeriesType::Standard,
        titles: vec!["Breaking Bad".into()],
    };
    let found = r
        .app
        .cardigann
        .search_tv(
            &provider,
            &TvSearch::Episode {
                series: series.clone(),
                season: 1,
                episode: 2,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        found.len(),
        2,
        "the fake site answers every search the same way"
    );
    r.app
        .cardigann
        .search_tv(&provider, &TvSearch::Season { series, season: 3 })
        .await
        .unwrap();
    let asked = seen.lock().unwrap().clone();
    assert!(
        asked.iter().any(|p| p.contains("q=Breaking+Bad+S01E02")),
        "{asked:?}"
    );
    assert!(
        asked.iter().any(|p| p.contains("q=Breaking+Bad+S03")),
        "{asked:?}"
    );
}

/// A site behind Cloudflare's check, and a FlareSolverr that gets past it.
async fn cloudflare_site_and_solver() -> (String, String) {
    use axum::{
        Json,
        http::HeaderMap,
        routing::{get, post},
    };
    let site = Router::new().route(
        "/browse",
        get(|headers: HeaderMap| async move {
            if headers
                .get("cookie")
                .and_then(|c| c.to_str().ok())
                .is_some_and(|c| c.contains("cf_clearance=ok"))
            {
                (
                    StatusCode::OK,
                    [("content-type", "text/html")],
                    RESULTS.to_owned(),
                )
            } else {
                (
                    StatusCode::FORBIDDEN,
                    [("content-type", "text/html")],
                    "<title>Just a moment...</title>".to_owned(),
                )
            }
        }),
    );
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let site_url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, site).await.unwrap() });

    let solver = Router::new()
        .route(
            "/",
            get(|| async { Json(json!({"msg": "FlareSolverr is ready!", "version": "3.3.0"})) }),
        )
        .route(
            "/v1",
            post(|Json(ask): Json<Value>| async move {
                assert_eq!(ask["cmd"], "request.get");
                Json(json!({"status": "ok", "solution": {
                    "url": ask["url"], "status": 200, "response": RESULTS,
                    "cookies": [{"name": "cf_clearance", "value": "ok"}],
                    "userAgent": "Solved/1.0"
                }}))
            }),
        );
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let solver_url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, solver).await.unwrap() });
    (site_url, solver_url)
}

#[tokio::test]
async fn a_site_behind_cloudflare_is_read_through_flaresolverr() {
    let (base, solver) = cloudflare_site_and_solver().await;
    let r = rig(&base).await;
    let (_, saved, _) = call(&r, Method::POST, "/api/v1/indexer", Some(site(json!({})))).await;
    let provider = r
        .app
        .db
        .providers()
        .get(saved["id"].as_i64().unwrap())
        .await
        .unwrap();

    // Without help the check is named as the problem.
    let err = r.app.cardigann.test(&provider).await.unwrap_err();
    assert!(err.contains("Cloudflare"), "{err}");

    // Something that is not FlareSolverr is refused; the real address is accepted.
    let (s, _, _) = call(
        &r,
        Method::PUT,
        "/api/v1/indexer/flaresolverr",
        Some(json!({"url": base})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, _) = call(
        &r,
        Method::PUT,
        "/api/v1/indexer/flaresolverr",
        Some(json!({"url": solver})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        call(&r, Method::GET, "/api/v1/indexer/flaresolverr", None)
            .await
            .1["url"],
        solver
    );

    r.app.cardigann.test(&provider).await.unwrap();
    let found = r.app.cardigann.recent(&provider, false).await.unwrap();
    assert_eq!(found.len(), 2);
}
