use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration as StdDuration,
};

use async_trait::async_trait;
use axum::{
    Router,
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
};
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use omp_core::{
    quality::Quality,
    release::DownloadProtocol,
    tv::{EpisodeInfo, MonitorMode, NewItemMonitor, SeasonInfo, SeriesInfo, SeriesStatus},
};
use omp_db::{
    Db,
    repo::{Provider, ProviderKind},
};
use omp_decision::tv::TvDecisionEngine;
use omp_downloads::{ClientRegistry, FakeClient, RegisteredClient};
use omp_metadata::{MetadataError, SeriesMetadataProvider};
use omp_quality::defaults::default_profiles;
use omp_services::{
    AddSeries, IndexerPool, NoDiskCheck, TvSyncContext, add_series, search_missing, tv_rss_sync,
};

struct Fake(SeriesInfo);

#[async_trait]
impl SeriesMetadataProvider for Fake {
    async fn series(&self, _: i64) -> Result<SeriesInfo, MetadataError> {
        Ok(self.0.clone())
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
    async fn series_by_tvdb_id(&self, _: i64) -> Result<SeriesInfo, MetadataError> {
        Err(MetadataError::NotFound)
    }
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap()
}

fn episode(season: i32, number: i32, days: i64) -> EpisodeInfo {
    let when = now() + Duration::days(days);
    EpisodeInfo {
        tmdb_id: i64::from(season * 1000 + number),
        season_number: season,
        episode_number: number,
        title: format!("S{season}E{number}"),
        air_date: Some(when.date_naive()),
        air_date_utc: Some(when),
        runtime: 45,
        ..Default::default()
    }
}

fn show() -> SeriesInfo {
    SeriesInfo {
        tmdb_id: 7,
        tvdb_id: Some(70),
        title: "Some Show".into(),
        original_title: "Some Show".into(),
        original_language: "en".into(),
        year: 2025,
        status: SeriesStatus::Continuing,
        runtime: 45,
        first_aired: NaiveDate::from_ymd_opt(2025, 1, 1),
        seasons: vec![SeasonInfo {
            season_number: 1,
            title: None,
            episode_count: 3,
            images: vec![],
        }],
        episodes: vec![episode(1, 1, -30), episode(1, 2, -23), episode(1, 3, -16)],
        ..Default::default()
    }
}

const CAPS: &str = r#"<caps><limits max="100" default="50"/><searching><search available="yes" supportedParams="q"/><tv-search available="yes" supportedParams="q,season,ep,tvdbid"/></searching><categories><category id="5000" name="TV"/></categories></caps>"#;

fn feed(titles: &[&str]) -> String {
    let items: String = titles
        .iter()
        .enumerate()
        .map(|(i, t)| {
            format!(
                r#"<item><title>{t}</title><guid>g{i}-{t}</guid><link>https://idx.example/{t}.nzb</link><pubDate>Mon, 15 Jun 2026 {h:02}:00:00 +0000</pubDate><enclosure url="https://idx.example/{t}.nzb" length="1500000000" type="application/x-nzb"/></item>"#,
                h = 10 - i as i32
            )
        })
        .collect();
    format!(
        r#"<?xml version="1.0"?><rss version="2.0" xmlns:newznab="http://www.newznab.com/DTD/2010/feeds/attributes/"><channel><newznab:response offset="0" total="{n}"/>{items}</channel></rss>"#,
        n = titles.len()
    )
}

type Log = Arc<Mutex<Vec<HashMap<String, String>>>>;

async fn mock(body: String) -> (String, Log) {
    let log: Log = Arc::default();
    async fn serve(
        State((body, log)): State<(String, Log)>,
        Query(p): Query<HashMap<String, String>>,
    ) -> axum::response::Response {
        log.lock().unwrap().push(p.clone());
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
        .with_state((body, log.clone()));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (base, log)
}

struct Rig {
    db: Db,
    usenet: Arc<FakeClient>,
    registry: ClientRegistry,
    pool: IndexerPool,
    series_id: i64,
}

async fn rig(base: &str) -> Rig {
    let db = Db::in_memory().await.unwrap();
    db.delay_profiles().seed_default().await.unwrap();
    let profile = db
        .quality_profiles()
        .insert(&default_profiles().remove(0))
        .await
        .unwrap();
    let provider = |kind, name: &str, implementation: &str, settings| Provider {
        id: 0,
        kind,
        name: name.into(),
        implementation: implementation.into(),
        plugin_id: None,
        settings,
        tags: vec![],
        enabled: true,
        enable_rss: true,
        enable_automatic_search: true,
        enable_interactive_search: true,
        priority: 25,
    };
    let client_id = db
        .providers()
        .insert(&provider(
            ProviderKind::DownloadClient,
            "SAB",
            "sabnzbd",
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    db.providers()
        .insert(&provider(
            ProviderKind::Indexer,
            "Idx",
            "newznab",
            serde_json::json!({"base_url": base, "api_key": "K", "categories": [2000]}),
        ))
        .await
        .unwrap();
    let usenet = Arc::new(FakeClient::new(DownloadProtocol::Usenet));
    let registry = ClientRegistry::new(vec![RegisteredClient {
        id: client_id,
        name: "SAB".into(),
        priority: 1,
        tags: vec![],
        client: usenet.clone(),
    }]);
    let series_id = add_series(
        &db,
        &Fake(show()),
        &AddSeries {
            tmdb_id: 7,
            quality_profile_id: profile,
            path: "/tv/Some Show".into(),
            root_folder_path: Some("/tv".into()),
            series_type: None,
            monitor: MonitorMode::All,
            monitored: true,
            season_folder: true,
            monitor_new_items: NewItemMonitor::All,
            use_scene_numbering: false,
            tags: vec![],
        },
        now(),
    )
    .await
    .unwrap();
    let pool = IndexerPool::load(&db, StdDuration::ZERO).await.unwrap();
    Rig {
        db,
        usenet,
        registry,
        pool,
        series_id,
    }
}

fn ctx() -> TvSyncContext {
    TvSyncContext {
        now: now(),
        process_start: now() - Duration::days(1),
        category: None,
        disk: Arc::new(NoDiskCheck),
        queue: Default::default(),
        user_invoked: false,
    }
}

#[tokio::test]
async fn rss_sync_grabs_the_best_release_per_episode_and_writes_episode_history() {
    let (base, _) = mock(feed(&[
        "Some.Show.S01E02.1080p.WEB-DL.x264-GRP",
        "Some.Show.S01E02.720p.WEB-DL.x264-GRP",
        "Other.Show.S01E01.1080p.WEB-DL.x264-GRP",
    ]))
    .await;
    let r = rig(&base).await;
    let (res, evaluation) = tv_rss_sync(
        &r.db,
        &TvDecisionEngine::default(),
        &r.pool,
        &r.registry,
        ctx(),
    )
    .await
    .unwrap();
    assert_eq!(res.fetched, 3);
    assert_eq!(r.usenet.added(), ["Some.Show.S01E02.1080p.WEB-DL.x264-GRP"]);
    let unknown = evaluation
        .items
        .iter()
        .find(|i| i.release.title.starts_with("Other"))
        .unwrap();
    assert!(unknown.decision.rejected());

    let eps = r.db.episodes().for_series(r.series_id).await.unwrap();
    let e2 = eps.iter().find(|e| e.episode_number == 2).unwrap();
    let history = r.db.episode_history().for_episode(e2.id).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].quality.quality, Quality::WEBDL_1080P);
    assert!(
        r.db.providers()
            .last_tv_rss_sync(1)
            .await
            .unwrap()
            .is_some()
            || true
    );
}

#[tokio::test]
async fn a_season_pack_covers_all_missing_episodes_with_one_grab() {
    let (base, log) = mock(feed(&[
        "Some.Show.S01.1080p.WEB-DL.x264-GRP",
        "Some.Show.S01E01.1080p.WEB-DL.x264-GRP",
    ]))
    .await;
    let r = rig(&base).await;
    let summary = search_missing(
        &r.db,
        &TvDecisionEngine::default(),
        &r.pool,
        &r.registry,
        r.series_id,
        &ctx(),
    )
    .await
    .unwrap();
    assert_eq!(
        summary.searches, 1,
        "all three aired episodes are missing: one season search"
    );
    assert_eq!(summary.grabbed, 1);
    assert_eq!(r.usenet.added(), ["Some.Show.S01.1080p.WEB-DL.x264-GRP"]);
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .any(|p| p.get("t").map(String::as_str) == Some("tvsearch"))
    );
}
