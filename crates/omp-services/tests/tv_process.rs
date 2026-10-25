use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use omp_core::{
    history::HistoryEvent,
    quality::{Quality, QualityModel},
    release::DownloadProtocol,
    tv::{EpisodeInfo, MonitorMode, NewItemMonitor, SeasonInfo, SeriesInfo, SeriesStatus},
};
use omp_db::{
    Db,
    repo::{EpisodeHistoryRecord, Provider, ProviderKind},
};
use omp_downloads::{ClientItem, ClientRegistry, DownloadItemStatus, FakeClient, RegisteredClient};
use omp_metadata::{MetadataError, SeriesMetadataProvider};
use omp_quality::defaults::default_profiles;
use omp_services::{
    AddSeries, DownloadMonitor, ImportConfig, ImportMode, MediaProbe, NoDiskCheck, ProbeInfo,
    ProcessOptions, TrackedState, add_series, process_downloads,
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

struct Probe;

#[async_trait]
impl MediaProbe for Probe {
    async fn probe(&self, _: &Path) -> Option<ProbeInfo> {
        Some(ProbeInfo {
            runtime_seconds: 2700.0,
            audio_streams: 1,
            raw: serde_json::json!({}),
        })
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

struct Rig {
    db: Db,
    dir: tempfile::TempDir,
    client: Arc<FakeClient>,
    registry: ClientRegistry,
    series: i64,
    episode_ids: Vec<i64>,
}

async fn rig() -> Rig {
    let db = Db::in_memory().await.unwrap();
    let profile = db
        .quality_profiles()
        .insert(
            &default_profiles()
                .into_iter()
                .find(|p| p.name == "HD-1080p")
                .unwrap(),
        )
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let info = SeriesInfo {
        tmdb_id: 7,
        title: "Some Show".into(),
        original_language: "en".into(),
        year: 2025,
        status: SeriesStatus::Continuing,
        runtime: 45,
        first_aired: NaiveDate::from_ymd_opt(2025, 1, 1),
        seasons: vec![SeasonInfo {
            season_number: 1,
            title: None,
            episode_count: 2,
            images: vec![],
        }],
        episodes: vec![episode(1), episode(2)],
        ..Default::default()
    };
    let series = add_series(
        &db,
        &Fake(info),
        &AddSeries {
            tmdb_id: 7,
            quality_profile_id: profile,
            path: dir
                .path()
                .join("tv/Some Show")
                .to_string_lossy()
                .into_owned(),
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
    let client_id = db
        .providers()
        .insert(&Provider {
            id: 0,
            kind: ProviderKind::DownloadClient,
            name: "SAB".into(),
            implementation: "sabnzbd".into(),
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
        .unwrap();
    let client = Arc::new(FakeClient::new(DownloadProtocol::Usenet));
    let registry = ClientRegistry::new(vec![RegisteredClient {
        id: client_id,
        name: "SAB".into(),
        priority: 1,
        tags: vec![],
        client: client.clone(),
    }]);
    let episode_ids = db
        .episodes()
        .for_series(series)
        .await
        .unwrap()
        .iter()
        .map(|e| e.id)
        .collect();
    Rig {
        db,
        dir,
        client,
        registry,
        series,
        episode_ids,
    }
}

async fn record_grab(r: &Rig, download_id: &str, title: &str) {
    let data = [
        ("indexer".to_owned(), "Idx".to_owned()),
        ("size".to_owned(), "1500000000".to_owned()),
        ("published_date".to_owned(), now().to_rfc3339()),
    ];
    for id in &r.episode_ids {
        r.db.episode_history()
            .insert(&EpisodeHistoryRecord {
                id: 0,
                series_id: r.series,
                episode_id: *id,
                event: HistoryEvent::Grabbed,
                date: now() - Duration::hours(1),
                source_title: title.into(),
                quality: QualityModel {
                    quality: Quality::WEBDL_1080P,
                    ..Default::default()
                },
                languages: vec![],
                download_id: Some(download_id.into()),
                data: data.clone().into(),
            })
            .await
            .unwrap();
    }
}

fn client_item(
    id: &str,
    title: &str,
    status: DownloadItemStatus,
    path: Option<String>,
) -> ClientItem {
    ClientItem {
        download_id: id.into(),
        category: Some("omp".into()),
        title: title.into(),
        total_size: 100,
        remaining_size: 0,
        status,
        output_path: path,
        seed_ratio: None,
        can_move_files: true,
        can_be_removed: false,
        message: Some("par2 repair failed".into()),
        ..Default::default()
    }
}

fn options() -> ProcessOptions {
    ProcessOptions {
        now: now(),
        category: Some("omp".into()),
        remove_failed: true,
        remove_completed: false,
    }
}

#[tokio::test]
async fn a_finished_season_pack_download_is_imported_for_every_episode() {
    let r = rig().await;
    let title = "Some.Show.S01.1080p.WEB-DL.x264-GRP";
    record_grab(&r, "d1", title).await;
    let folder = r.dir.path().join("downloads").join(title);
    std::fs::create_dir_all(&folder).unwrap();
    for n in [1, 2] {
        std::fs::write(
            folder.join(format!("Some.Show.S01E0{n}.1080p.WEB-DL.x264-GRP.mkv")),
            b"x",
        )
        .unwrap();
    }
    r.client.set_items(vec![client_item(
        "d1",
        title,
        DownloadItemStatus::Completed,
        Some(folder.to_string_lossy().into_owned()),
    )]);

    let monitor = DownloadMonitor::new();
    let mut config = ImportConfig {
        mode: ImportMode::Move,
        ..Default::default()
    };
    config.naming.rename_episodes = true;
    let result = process_downloads(
        &r.db,
        &monitor,
        &r.registry,
        &config,
        &Probe,
        &NoDiskCheck,
        &options(),
    )
    .await
    .unwrap();
    assert_eq!(result.imported, ["d1"], "{result:?}");

    let tracked = monitor.get("d1").unwrap();
    assert_eq!(tracked.state, TrackedState::Imported);
    assert_eq!(tracked.series_id, Some(r.series));
    assert_eq!(tracked.episode_ids.len(), 2);
    let eps = r.db.episodes().for_series(r.series).await.unwrap();
    assert!(eps.iter().all(|e| e.episode_file_id.is_some()));
    let h = r.db.episode_history().by_download_id("d1").await.unwrap();
    assert_eq!(
        h.iter()
            .filter(|x| x.event == HistoryEvent::DownloadFolderImported)
            .count(),
        2
    );
}

#[tokio::test]
async fn a_failed_download_is_recorded_and_blocklisted_per_series() {
    let r = rig().await;
    let title = "Some.Show.S01E01.1080p.WEB-DL.x264-GRP";
    record_grab(&r, "d2", title).await;
    r.client.set_items(vec![client_item(
        "d2",
        title,
        DownloadItemStatus::Failed,
        None,
    )]);

    let monitor = DownloadMonitor::new();
    let result = process_downloads(
        &r.db,
        &monitor,
        &r.registry,
        &ImportConfig::default(),
        &Probe,
        &NoDiskCheck,
        &options(),
    )
    .await
    .unwrap();
    assert_eq!(result.refresh.newly_failed, ["d2"]);
    assert_eq!(monitor.get("d2").unwrap().state, TrackedState::Failed);

    let blocked = r.db.tv_blocklist().for_series(r.series).await.unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].entry.source_title, title);
    assert_eq!(blocked[0].episode_ids.len(), 2);
    let h =
        r.db.episode_history()
            .for_episode(r.episode_ids[0])
            .await
            .unwrap();
    assert!(h.iter().any(|x| x.event == HistoryEvent::DownloadFailed));
    assert_eq!(r.client.removed(), [("d2".to_owned(), true)]);
}

#[tokio::test]
async fn a_client_path_is_translated_before_importing() {
    let r = rig().await;
    let title = "Some.Show.S01E01.1080p.WEB-DL.x264-GRP";
    let local = r.dir.path().join("mnt").join(title);
    std::fs::create_dir_all(&local).unwrap();
    std::fs::write(local.join(format!("{title}.mkv")), b"x").unwrap();
    r.db.remote_path_mappings()
        .insert(
            None,
            "/remote/dl",
            &r.dir.path().join("mnt").to_string_lossy(),
        )
        .await
        .unwrap();
    record_grab(&r, "d3", title).await;
    r.client.set_items(vec![client_item(
        "d3",
        title,
        DownloadItemStatus::Completed,
        Some(format!("/remote/dl/{title}")),
    )]);
    let monitor = DownloadMonitor::new();
    let config = ImportConfig {
        mode: ImportMode::Move,
        ..Default::default()
    };
    let result = process_downloads(
        &r.db,
        &monitor,
        &r.registry,
        &config,
        &Probe,
        &NoDiskCheck,
        &options(),
    )
    .await
    .unwrap();
    assert_eq!(result.imported, ["d3"], "{result:?}");
    assert_eq!(
        monitor.get("d3").unwrap().item.output_path.unwrap(),
        local.to_string_lossy()
    );
}
