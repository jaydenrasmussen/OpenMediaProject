use std::{
    collections::{HashMap, HashSet},
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
    release::DownloadProtocol,
    tv::{EpisodeInfo, MonitorMode, NewItemMonitor, SeasonInfo, SeriesInfo, SeriesStatus},
};
use omp_db::{
    Db,
    repo::{Provider, ProviderKind, SeriesUpdate},
};
use omp_decision::tv::TvDecisionEngine;
use omp_downloads::{ClientRegistry, FakeClient, RegisteredClient};
use omp_metadata::{
    MetadataError, SeriesMetadataProvider,
    scene::{Numbering, SceneSource, SceneTitle, XemEpisode},
};
use omp_quality::defaults::default_profiles;
use omp_services::{
    AddSeries, IndexerPool, NoDiskCheck, TvSyncContext, add_series, refresh_scene_data,
    search_episode, tv_rss_sync,
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

struct Scene {
    known: Mutex<HashSet<i64>>,
}

#[async_trait]
impl SceneSource for Scene {
    async fn scene_titles(&self) -> Result<Vec<SceneTitle>, MetadataError> {
        Ok(vec![SceneTitle {
            tvdb_id: 70,
            title: "Shortname".into(),
            search_title: "Shortname".into(),
            season: -1,
        }])
    }
    async fn xem_known_ids(&self) -> Result<HashSet<i64>, MetadataError> {
        Ok(self.known.lock().unwrap().clone())
    }
    async fn xem_map(&self, _: i64) -> Result<Vec<XemEpisode>, MetadataError> {
        // The provider's S01E03 is S02E01 to scene releases.
        Ok(vec![XemEpisode {
            tvdb: Numbering {
                season: 1,
                episode: 3,
                absolute: None,
            },
            scene: Numbering {
                season: 2,
                episode: 1,
                absolute: None,
            },
        }])
    }
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap()
}

fn episode(number: i32) -> EpisodeInfo {
    let when = now() - Duration::days(30 - i64::from(number));
    EpisodeInfo {
        tmdb_id: i64::from(number),
        season_number: 1,
        episode_number: number,
        title: format!("Title {number}"),
        air_date: Some(when.date_naive()),
        air_date_utc: Some(when),
        runtime: 45,
        ..Default::default()
    }
}

const CAPS: &str = r#"<caps><limits max="100" default="50"/><searching><search available="yes" supportedParams="q"/><tv-search available="yes" supportedParams="q,season,ep,tvdbid"/></searching><categories><category id="5000" name="TV"/></categories></caps>"#;

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

fn feed(titles: &[&str]) -> String {
    let items: String = titles.iter().enumerate().map(|(i, t)| format!(
        r#"<item><title>{t}</title><guid>g{i}-{t}</guid><link>https://idx.example/{t}.nzb</link><pubDate>Mon, 15 Jun 2026 10:00:00 +0000</pubDate><enclosure url="https://idx.example/{t}.nzb" length="1500000000" type="application/x-nzb"/></item>"#)).collect();
    format!(
        r#"<?xml version="1.0"?><rss version="2.0" xmlns:newznab="http://www.newznab.com/DTD/2010/feeds/attributes/"><channel><newznab:response offset="0" total="{n}"/>{items}</channel></rss>"#,
        n = titles.len()
    )
}

struct Rig {
    db: Db,
    usenet: Arc<FakeClient>,
    registry: ClientRegistry,
    pool: IndexerPool,
    series: i64,
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
    let info = SeriesInfo {
        tmdb_id: 7,
        tvdb_id: Some(70),
        title: "Some Show".into(),
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
        episodes: vec![episode(1), episode(2), episode(3)],
        ..Default::default()
    };
    let series = add_series(
        &db,
        &Fake(info),
        &AddSeries {
            tmdb_id: 7,
            quality_profile_id: profile,
            path: "/tv/Some Show".into(),
            root_folder_path: None,
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
        series,
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

async fn follow_scene(r: &Rig) {
    let s = r.db.series().get(r.series).await.unwrap();
    r.db.series()
        .update(
            r.series,
            &SeriesUpdate {
                series_type: s.series_type,
                monitored: s.monitored,
                monitor_new_items: s.monitor_new_items,
                quality_profile_id: s.quality_profile_id,
                season_folder: s.season_folder,
                use_scene_numbering: true,
                path: s.path.clone(),
                root_folder_path: s.root_folder_path.clone(),
                tags: s.tags.clone(),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn xem_numbers_are_stored_and_cleared_when_the_mapping_goes() {
    let (base, _) = mock(feed(&[])).await;
    let r = rig(&base).await;
    let source = Scene {
        known: Mutex::new([70].into_iter().collect()),
    };
    let done = refresh_scene_data(&r.db, &source).await.unwrap();
    assert_eq!(
        (
            done.titles,
            done.renumbered_series,
            done.renumbered_episodes
        ),
        (1, 1, 1)
    );
    let eps = r.db.episodes().for_series(r.series).await.unwrap();
    assert_eq!(
        (eps[2].scene_season_number, eps[2].scene_episode_number),
        (Some(2), Some(1))
    );
    assert_eq!(eps[0].scene_season_number, None);
    assert_eq!(r.db.scene_titles().for_tvdb_id(70).await.unwrap().len(), 1);

    source.known.lock().unwrap().clear();
    refresh_scene_data(&r.db, &source).await.unwrap();
    assert_eq!(
        r.db.episodes().for_series(r.series).await.unwrap()[2].scene_season_number,
        None
    );
}

#[tokio::test]
async fn a_release_with_scene_numbers_and_an_alternate_title_matches() {
    let (base, _) = mock(feed(&[
        "Shortname.S02E01.1080p.WEB-DL.x264-GRP",
        "Shortname.S01E03.720p.WEB-DL.x264-GRP",
    ]))
    .await;
    let r = rig(&base).await;
    refresh_scene_data(
        &r.db,
        &Scene {
            known: Mutex::new([70].into_iter().collect()),
        },
    )
    .await
    .unwrap();

    // Not following scene numbering: S02E01 means nothing.
    let (_, ev) = tv_rss_sync(
        &r.db,
        &TvDecisionEngine::default(),
        &r.pool,
        &r.registry,
        ctx(),
    )
    .await
    .unwrap();
    let scene_release = ev
        .items
        .iter()
        .find(|i| i.release.title.contains("S02E01"))
        .unwrap();
    assert!(
        scene_release.series.is_some(),
        "the alternate title finds the series"
    );
    assert!(scene_release.episodes.is_empty());
    assert!(r.usenet.added().len() <= 1);

    follow_scene(&r).await;
    let (_, ev) = tv_rss_sync(
        &r.db,
        &TvDecisionEngine::default(),
        &r.pool,
        &r.registry,
        ctx(),
    )
    .await
    .unwrap();
    let scene_release = ev
        .items
        .iter()
        .find(|i| i.release.title.contains("S02E01"))
        .unwrap();
    assert_eq!(scene_release.episodes.len(), 1);
    assert_eq!(
        scene_release.episodes[0].episode_number, 3,
        "S02E01 is the provider's S01E03"
    );
}

#[tokio::test]
async fn searches_use_scene_numbers_and_titles() {
    let (base, log) = mock(feed(&[])).await;
    let r = rig(&base).await;
    refresh_scene_data(
        &r.db,
        &Scene {
            known: Mutex::new([70].into_iter().collect()),
        },
    )
    .await
    .unwrap();
    follow_scene(&r).await;
    let eps = r.db.episodes().for_series(r.series).await.unwrap();
    search_episode(
        &r.db,
        &TvDecisionEngine::default(),
        &r.pool,
        &r.registry,
        eps[2].id,
        omp_services::SearchMode::Automatic,
        &ctx(),
    )
    .await
    .unwrap();
    let asked: Vec<_> = log
        .lock()
        .unwrap()
        .iter()
        .filter(|p| p.get("t").map(String::as_str) == Some("tvsearch"))
        .cloned()
        .collect();
    assert!(
        asked
            .iter()
            .any(|p| p.get("season").map(String::as_str) == Some("2")
                && p.get("ep").map(String::as_str) == Some("1")),
        "{asked:?}"
    );
}
