use std::{
    sync::{Arc, Mutex},
    time::Duration as StdDuration,
};

use axum::{
    Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use chrono::{DateTime, Duration, TimeZone, Utc};
use omp_core::{
    language::Language,
    movie::{MovieMetadata, MovieStatus},
    quality::Quality,
    release::DownloadProtocol,
};
use omp_db::{
    Db,
    repo::{NewMovie, Provider, ProviderKind},
};
use omp_decision::DecisionEngine;
use omp_downloads::{ClientRegistry, FakeClient, RegisteredClient};
use omp_quality::profile::{ProfileItem, QualityProfile};
use omp_services::{
    IndexerPool, NoDiskCheck, QueueMap, SearchMode, SyncContext, rss_sync, search_movie,
};
use std::collections::HashMap;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap()
}

async fn setup() -> Db {
    let db = Db::in_memory().await.unwrap();
    db.delay_profiles().seed_default().await.unwrap();
    let q = |quality, allowed| ProfileItem::Quality { quality, allowed };
    db.quality_profiles()
        .insert(&QualityProfile {
            id: 0,
            name: "HD".into(),
            cutoff: Quality::BLURAY_1080P.id,
            items: vec![
                q(Quality::DVD, true),
                q(Quality::WEBDL_720P, true),
                q(Quality::WEBDL_1080P, true),
                q(Quality::BLURAY_1080P, true),
            ],
            min_format_score: 0,
            cutoff_format_score: 0,
            min_upgrade_format_score: 1,
            format_items: vec![],
            language: Language::ANY,
            upgrade_allowed: true,
        })
        .await
        .unwrap();
    db
}

async fn add_movie(db: &Db, tmdb: i64, title: &str, year: i32) -> i64 {
    let profile = db.quality_profiles().list().await.unwrap()[0].id;
    let metadata_id = db
        .movies()
        .upsert_metadata(&MovieMetadata {
            tmdb_id: tmdb,
            title: title.into(),
            original_title: title.into(),
            original_language: "en".into(),
            year,
            runtime: 120,
            status: MovieStatus::Released,
            ..Default::default()
        })
        .await
        .unwrap();
    db.movies()
        .add(&NewMovie {
            metadata_id,
            monitored: true,
            minimum_availability: MovieStatus::Announced,
            quality_profile_id: profile,
            path: format!("/movies/{title}"),
            root_folder_path: None,
            tags: vec![],
        })
        .await
        .unwrap()
}

async fn client_row(db: &Db, name: &str, protocol: &str) -> i64 {
    db.providers()
        .insert(&Provider {
            id: 0,
            kind: ProviderKind::DownloadClient,
            name: name.into(),
            implementation: protocol.into(),
            plugin_id: None,
            settings: serde_json::json!({}),
            tags: vec![],
            enabled: true,
            enable_rss: false,
            enable_automatic_search: false,
            enable_interactive_search: false,
            priority: 1,
        })
        .await
        .unwrap()
}

const CAPS: &str = r#"<caps><limits max="100" default="50"/><searching><search available="yes" supportedParams="q"/><movie-search available="yes" supportedParams="q,tmdbid,imdbid"/></searching><categories><category id="2000" name="Movies"/></categories></caps>"#;

fn feed(titles: &[&str]) -> String {
    let items: String = titles
        .iter()
        .enumerate()
        .map(|(i, t)| format!(
            r#"<item><title>{t}</title><guid>g{i}-{t}</guid><link>https://idx.example/{t}.nzb</link><pubDate>Mon, 15 Jun 2026 {h:02}:00:00 +0000</pubDate><enclosure url="https://idx.example/{t}.nzb" length="4000000000" type="application/x-nzb"/></item>"#,
            h = 10 - i as i32
        ))
        .collect();
    format!(
        r#"<?xml version="1.0"?><rss version="2.0" xmlns:newznab="http://www.newznab.com/DTD/2010/feeds/attributes/"><channel><newznab:response offset="0" total="{n}"/>{items}</channel></rss>"#,
        n = titles.len()
    )
}

type Log = Arc<Mutex<Vec<HashMap<String, String>>>>;

/// Serves caps and a fixed feed, or a 500 when `fail` is set.
async fn mock(body: String, fail: bool) -> (String, Log) {
    let log: Log = Arc::default();
    async fn serve(
        State((body, fail, log)): State<(String, bool, Log)>,
        Query(p): Query<HashMap<String, String>>,
    ) -> axum::response::Response {
        log.lock().unwrap().push(p.clone());
        if fail {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        let out = if p.get("t").map(String::as_str) == Some("caps") {
            CAPS.to_owned()
        } else {
            body
        };
        (
            [(axum::http::header::CONTENT_TYPE, "application/rss+xml")],
            out,
        )
            .into_response()
    }
    let app = Router::new()
        .route("/api", get(serve))
        .with_state((body, fail, log.clone()));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (base, log)
}

async fn indexer_row(db: &Db, base: &str) -> i64 {
    db.providers()
        .insert(&Provider {
            id: 0,
            kind: ProviderKind::Indexer,
            name: "Idx".into(),
            implementation: "newznab".into(),
            plugin_id: None,
            settings: serde_json::json!({"base_url": base, "api_key": "K", "categories": [2000]}),
            tags: vec![],
            enabled: true,
            enable_rss: true,
            enable_automatic_search: true,
            enable_interactive_search: true,
            priority: 25,
        })
        .await
        .unwrap()
}

fn sync_ctx() -> SyncContext {
    SyncContext {
        now: now(),
        process_start: now() - Duration::days(1),
        category: None,
        disk: Arc::new(NoDiskCheck),
        queue: QueueMap::new(),
        user_invoked: false,
    }
}

struct Rig {
    db: Db,
    usenet: Arc<FakeClient>,
    registry: ClientRegistry,
    indexer: i64,
}

async fn rig(base: &str) -> Rig {
    let db = setup().await;
    let usenet = Arc::new(FakeClient::new(DownloadProtocol::Usenet));
    let id = client_row(&db, "SAB", "sabnzbd").await;
    let registry = ClientRegistry::new(vec![RegisteredClient {
        id,
        name: "SAB".into(),
        priority: 1,
        tags: vec![],
        client: usenet.clone(),
    }]);
    let indexer = indexer_row(&db, base).await;
    Rig {
        db,
        usenet,
        registry,
        indexer,
    }
}

async fn pool(db: &Db) -> IndexerPool {
    IndexerPool::load(db, StdDuration::ZERO).await.unwrap()
}

#[tokio::test]
async fn rss_sync_grabs_the_best_release_and_remembers_where_it_stopped() {
    let (base, log) = mock(
        feed(&[
            "The.Matrix.1999.1080p.BluRay.x264-GRP",
            "The.Matrix.1999.720p.WEB-DL.x264-GRP",
        ]),
        false,
    )
    .await;
    let r = rig(&base).await;
    add_movie(&r.db, 603, "The Matrix", 1999).await;
    let pool = pool(&r.db).await;
    assert_eq!(pool.len(), 1);

    let res = rss_sync(
        &r.db,
        &DecisionEngine::default(),
        &pool,
        &r.registry,
        sync_ctx(),
    )
    .await
    .unwrap();
    assert_eq!(res.fetched, 2);
    assert!(res.failures.is_empty());
    assert_eq!(r.usenet.added(), ["The.Matrix.1999.1080p.BluRay.x264-GRP"]);
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .any(|p| p.get("t").map(String::as_str) == Some("movie")
                || p.get("t").map(String::as_str) == Some("search"))
    );

    let seen =
        r.db.providers()
            .last_rss_sync(r.indexer)
            .await
            .unwrap()
            .expect("stored");
    assert_eq!(seen.0, Utc.with_ymd_and_hms(2026, 6, 15, 10, 0, 0).unwrap());
}

#[tokio::test]
async fn rss_sync_failure_backs_the_indexer_off_and_skips_it_next_time() {
    let (base, log) = mock(String::new(), true).await;
    let r = rig(&base).await;
    add_movie(&r.db, 603, "The Matrix", 1999).await;
    let pool = pool(&r.db).await;

    let res = rss_sync(
        &r.db,
        &DecisionEngine::default(),
        &pool,
        &r.registry,
        sync_ctx(),
    )
    .await
    .unwrap();
    assert_eq!(res.failures.len(), 1);
    assert_eq!(res.fetched, 0);
    assert!(
        r.db.providers()
            .status(r.indexer)
            .await
            .unwrap()
            .is_disabled(now())
    );

    let before = log.lock().unwrap().len();
    let res = rss_sync(
        &r.db,
        &DecisionEngine::default(),
        &pool,
        &r.registry,
        sync_ctx(),
    )
    .await
    .unwrap();
    assert!(res.failures.is_empty(), "blocked indexers are not called");
    assert_eq!(log.lock().unwrap().len(), before);
}

#[tokio::test]
async fn automatic_search_grabs_and_interactive_does_not() {
    let (base, _) = mock(feed(&["The.Matrix.1999.1080p.BluRay.x264-GRP"]), false).await;
    let r = rig(&base).await;
    let id = add_movie(&r.db, 603, "The Matrix", 1999).await;
    let pool = pool(&r.db).await;
    let engine = DecisionEngine::default();

    let res = search_movie(
        &r.db,
        &engine,
        &pool,
        &r.registry,
        id,
        SearchMode::Interactive,
        SyncContext {
            user_invoked: true,
            ..sync_ctx()
        },
    )
    .await
    .unwrap();
    assert!(res.processed.is_none());
    assert_eq!(res.evaluation.items.len(), 1);
    assert!(r.usenet.added().is_empty());

    let res = search_movie(
        &r.db,
        &engine,
        &pool,
        &r.registry,
        id,
        SearchMode::Automatic,
        sync_ctx(),
    )
    .await
    .unwrap();
    assert_eq!(res.processed.unwrap().grabbed.len(), 1);
    assert_eq!(r.usenet.added().len(), 1);
}
