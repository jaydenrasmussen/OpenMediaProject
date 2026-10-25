use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use omp_core::{
    history::HistoryEvent,
    tv::{EpisodeInfo, MonitorMode, NewItemMonitor, SeasonInfo, SeriesInfo, SeriesStatus},
};
use omp_db::Db;
use omp_metadata::{MetadataError, SeriesMetadataProvider};
use omp_quality::defaults::default_profiles;
use omp_services::{
    AddSeries, FileOutcome, ImportConfig, ImportMode, ImportRejection, MediaProbe, NoDiskCheck,
    ProbeInfo, TvImportRequest, TvImporter, add_series,
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

fn episode(season: i32, number: i32, days: i64) -> EpisodeInfo {
    let when = now() + Duration::days(days);
    EpisodeInfo {
        tmdb_id: i64::from(season * 1000 + number),
        season_number: season,
        episode_number: number,
        title: format!("Title {number}"),
        air_date: Some(when.date_naive()),
        air_date_utc: Some(when),
        runtime: 45,
        ..Default::default()
    }
}

fn show() -> SeriesInfo {
    SeriesInfo {
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
            episode_count: 4,
            images: vec![],
        }],
        episodes: (1..=4).map(|n| episode(1, n, -30 + i64::from(n))).collect(),
        ..Default::default()
    }
}

struct Lib {
    db: Db,
    dir: tempfile::TempDir,
    series: i64,
    config: ImportConfig,
}

impl Lib {
    async fn new() -> Self {
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
        let series = add_series(
            &db,
            &Fake(show()),
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
        let mut config = ImportConfig {
            mode: ImportMode::Move,
            ..Default::default()
        };
        config.naming.rename_episodes = true;
        Self {
            db,
            dir,
            series,
            config,
        }
    }

    fn drop_file(&self, folder: &str, name: &str) -> PathBuf {
        let d = self.dir.path().join("downloads").join(folder);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join(name);
        std::fs::write(&p, b"video").unwrap();
        p
    }

    async fn import(
        &self,
        folder: &str,
        title: &str,
        episode_ids: &[i64],
    ) -> omp_services::ImportResult {
        let path = self.dir.path().join("downloads").join(folder);
        TvImporter {
            db: &self.db,
            config: &self.config,
            probe: &Probe,
            disk: &NoDiskCheck,
        }
        .import(&TvImportRequest {
            series_id: self.series,
            output_path: &path,
            download_id: Some(folder),
            download_title: title,
            episode_ids,
            can_move_files: true,
            now: now(),
            manual: None,
        })
        .await
        .unwrap()
    }

    async fn episodes(&self) -> Vec<omp_db::repo::EpisodeRecord> {
        self.db.episodes().for_series(self.series).await.unwrap()
    }
}

#[tokio::test]
async fn a_season_pack_imports_each_episode_with_renaming() {
    let lib = Lib::new().await;
    let title = "Some.Show.S01.1080p.WEB-DL.x264-GRP";
    lib.drop_file(title, "Some.Show.S01E01.1080p.WEB-DL.x264-GRP.mkv");
    lib.drop_file(title, "Some.Show.S01E02.1080p.WEB-DL.x264-GRP.mkv");
    lib.drop_file(title, "readme.txt");
    let r = lib.import(title, title, &[]).await;
    assert_eq!(r.files.len(), 2);
    assert!(
        r.files
            .iter()
            .all(|f| matches!(f.outcome, FileOutcome::Imported { .. })),
        "{r:?}"
    );

    let series = lib.db.series().get(lib.series).await.unwrap();
    let season = Path::new(&series.path).join("Season 1");
    assert!(
        season
            .join("Some Show - S01E01 - Title 1 WEBDL-1080p.mkv")
            .exists()
    );
    assert!(
        season
            .join("Some Show - S01E02 - Title 2 WEBDL-1080p.mkv")
            .exists()
    );
    let eps = lib.episodes().await;
    assert_eq!(
        eps.iter().filter(|e| e.episode_file_id.is_some()).count(),
        2
    );
    let h = lib
        .db
        .episode_history()
        .for_episode(eps[0].id)
        .await
        .unwrap();
    assert_eq!(h[0].event, HistoryEvent::DownloadFolderImported);
}

#[tokio::test]
async fn a_multi_episode_file_covers_both_episodes() {
    let lib = Lib::new().await;
    let title = "Some.Show.S01E03E04.1080p.WEB-DL.x264-GRP";
    lib.drop_file(title, &format!("{title}.mkv"));
    let r = lib.import(title, title, &[]).await;
    assert!(
        matches!(r.files[0].outcome, FileOutcome::Imported { .. }),
        "{r:?}"
    );
    let series = lib.db.series().get(lib.series).await.unwrap();
    assert!(
        Path::new(&series.path)
            .join("Season 1/Some Show - S01E03-E04 - Title 3 + Title 4 WEBDL-1080p.mkv")
            .exists()
    );
    let eps = lib.episodes().await;
    let ids: Vec<_> = eps.iter().filter_map(|e| e.episode_file_id).collect();
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0], ids[1]);
}

#[tokio::test]
async fn upgrades_replace_and_equal_quality_is_refused() {
    let lib = Lib::new().await;
    let first = "Some.Show.S01E02.720p.WEB-DL.x264-GRP";
    lib.drop_file(first, &format!("{first}.mkv"));
    assert!(matches!(
        lib.import(first, first, &[]).await.files[0].outcome,
        FileOutcome::Imported { .. }
    ));

    let same = "Some.Show.S01E02.720p.WEB-DL.x264-OTHER";
    lib.drop_file(same, &format!("{same}.mkv"));
    let r = lib.import(same, same, &[]).await;
    assert!(
        matches!(&r.files[0].outcome, FileOutcome::Rejected(v) if matches!(v[0], ImportRejection::NotQualityUpgrade(_))),
        "{r:?}"
    );

    let better = "Some.Show.S01E02.1080p.BluRay.x264-GRP";
    lib.drop_file(better, &format!("{better}.mkv"));
    let r = lib.import(better, better, &[]).await;
    assert!(
        matches!(r.files[0].outcome, FileOutcome::Imported { .. }),
        "{r:?}"
    );
    let series = lib.db.series().get(lib.series).await.unwrap();
    let season = Path::new(&series.path).join("Season 1");
    assert!(
        !season
            .join("Some Show - S01E02 - Title 2 WEBDL-720p.mkv")
            .exists(),
        "the old file is gone"
    );
    assert!(
        season
            .join("Some Show - S01E02 - Title 2 Bluray-1080p.mkv")
            .exists()
    );
    assert_eq!(
        lib.db
            .episode_files()
            .for_series(lib.series)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_lone_file_with_an_unreadable_name_takes_the_grabbed_episode() {
    let lib = Lib::new().await;
    let title = "Some.Show.S01E04.1080p.WEB-DL.x264-GRP";
    lib.drop_file(title, "abc123.mkv");
    let eps = lib.episodes().await;
    let e4 = eps.iter().find(|e| e.episode_number == 4).unwrap();
    let r = lib.import(title, title, &[e4.id]).await;
    assert!(
        matches!(r.files[0].outcome, FileOutcome::Imported { .. }),
        "{r:?}"
    );
    assert!(
        lib.episodes()
            .await
            .iter()
            .find(|e| e.episode_number == 4)
            .unwrap()
            .episode_file_id
            .is_some()
    );

    // A file for an episode that wasn't grabbed is refused.
    let other = "Some.Show.S01E01.1080p.WEB-DL.x264-GRP";
    lib.drop_file(other, &format!("{other}.mkv"));
    let r = lib.import(other, other, &[e4.id]).await;
    assert!(
        matches!(r.files[0].outcome, FileOutcome::Rejected(_)),
        "{r:?}"
    );
}

#[tokio::test]
async fn rescan_records_existing_files_and_forgets_missing_ones() {
    let lib = Lib::new().await;
    let series = lib.db.series().get(lib.series).await.unwrap();
    let season = Path::new(&series.path).join("Season 1");
    std::fs::create_dir_all(&season).unwrap();
    std::fs::write(
        season.join("Some.Show.S01E01.720p.WEB-DL.x264-GRP.mkv"),
        b"x",
    )
    .unwrap();
    std::fs::write(
        season.join("Some.Show.S01E02E03.1080p.BluRay.x264-GRP.mkv"),
        b"x",
    )
    .unwrap();
    std::fs::write(season.join("random name.mkv"), b"x").unwrap();

    let r = omp_services::rescan_series(&lib.db, lib.series, now())
        .await
        .unwrap();
    assert_eq!((r.added, r.removed), (2, 0));
    assert_eq!(r.unmatched, ["Season 1/random name.mkv"]);
    let eps = lib.episodes().await;
    let with_file: Vec<_> = eps
        .iter()
        .filter(|e| e.episode_file_id.is_some())
        .map(|e| e.episode_number)
        .collect();
    assert_eq!(with_file, [1, 2, 3]);
    assert_eq!(eps[1].episode_file_id, eps[2].episode_file_id);

    // Scanning again changes nothing; deleting a file is noticed.
    let again = omp_services::rescan_series(&lib.db, lib.series, now())
        .await
        .unwrap();
    assert_eq!((again.added, again.removed), (0, 0));
    std::fs::remove_file(season.join("Some.Show.S01E01.720p.WEB-DL.x264-GRP.mkv")).unwrap();
    let r = omp_services::rescan_series(&lib.db, lib.series, now())
        .await
        .unwrap();
    assert_eq!((r.added, r.removed), (0, 1));
    assert!(lib.episodes().await[0].episode_file_id.is_none());
}

#[tokio::test]
async fn renaming_moves_files_to_the_naming_format() {
    let lib = Lib::new().await;
    let series = lib.db.series().get(lib.series).await.unwrap();
    std::fs::create_dir_all(&series.path).unwrap();
    std::fs::write(
        Path::new(&series.path).join("Some.Show.S01E01.1080p.WEB-DL.x264-GRP.mkv"),
        b"x",
    )
    .unwrap();
    omp_services::rescan_series(&lib.db, lib.series, now())
        .await
        .unwrap();

    let previews =
        omp_services::episode_rename_previews(&lib.db, &lib.config.naming, Some(lib.series))
            .await
            .unwrap();
    assert_eq!(previews.len(), 1);
    assert_eq!(
        previews[0].new,
        "Season 1/Some Show - S01E01 - Title 1 WEBDL-1080p.mkv"
    );

    let n = omp_services::rename_series(&lib.db, &lib.config.naming, lib.series, now())
        .await
        .unwrap();
    assert_eq!(n, 1);
    assert!(Path::new(&series.path).join(&previews[0].new).exists());
    assert!(
        omp_services::episode_rename_previews(&lib.db, &lib.config.naming, None)
            .await
            .unwrap()
            .is_empty(),
        "nothing left to rename"
    );
    let off = omp_naming::NamingConfig::default();
    assert!(
        omp_services::episode_rename_previews(&lib.db, &off, None)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn subtitles_come_along_when_that_is_switched_on() {
    let mut lib = Lib::new().await;
    let title = "Some.Show.S01E01.1080p.WEB-DL.x264-GRP";
    lib.drop_file(title, &format!("{title}.mkv"));
    lib.drop_file(title, &format!("{title}.en.srt"));
    lib.config.import_extra_files = true;
    let r = lib.import(title, title, &[]).await;
    assert!(
        matches!(r.files[0].outcome, FileOutcome::Imported { .. }),
        "{r:?}"
    );
    let series = lib.db.series().get(lib.series).await.unwrap();
    let season = Path::new(&series.path).join("Season 1");
    assert!(
        season
            .join("Some Show - S01E01 - Title 1 WEBDL-1080p.mkv")
            .exists()
    );
    assert!(
        season
            .join("Some Show - S01E01 - Title 1 WEBDL-1080p.en.srt")
            .exists()
    );
}
