use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{Duration, NaiveDate, TimeZone, Utc};
use omp_core::tv::{
    EpisodeInfo, MonitorMode, NewItemMonitor, SeasonInfo, SeriesInfo, SeriesStatus, SeriesType,
};
use omp_db::Db;
use omp_metadata::{MetadataError, SeriesMetadataProvider};
use omp_quality::defaults::default_profiles;
use omp_services::{AddSeries, add_series, apply_monitor_mode, refresh_series};

/// Serves whatever series it was last given.
struct FakeProvider {
    info: Mutex<SeriesInfo>,
}

impl FakeProvider {
    fn set(&self, info: SeriesInfo) {
        *self.info.lock().unwrap() = info;
    }
}

#[async_trait]
impl SeriesMetadataProvider for FakeProvider {
    async fn series(&self, tmdb_id: i64) -> Result<SeriesInfo, MetadataError> {
        let info = self.info.lock().unwrap().clone();
        if info.tmdb_id == tmdb_id {
            Ok(info)
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
    async fn series_by_tvdb_id(&self, _: i64) -> Result<SeriesInfo, MetadataError> {
        Err(MetadataError::NotFound)
    }
}

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap()
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
        title: "Some Show".into(),
        original_language: "en".into(),
        year: 2025,
        status: SeriesStatus::Continuing,
        first_aired: NaiveDate::from_ymd_opt(2025, 1, 1),
        seasons: vec![
            SeasonInfo {
                season_number: 0,
                title: Some("Specials".into()),
                episode_count: 1,
                images: vec![],
            },
            SeasonInfo {
                season_number: 1,
                title: None,
                episode_count: 2,
                images: vec![],
            },
            SeasonInfo {
                season_number: 2,
                title: None,
                episode_count: 2,
                images: vec![],
            },
        ],
        episodes: vec![
            episode(0, 1, -300),
            episode(1, 1, -200),
            episode(1, 2, -193),
            episode(2, 1, -30),
            episode(2, 2, 5),
        ],
        ..Default::default()
    }
}

fn request(profile: i32, monitor: MonitorMode) -> AddSeries {
    AddSeries {
        tmdb_id: 7,
        quality_profile_id: profile,
        path: "/tv/Some Show (2025)".into(),
        root_folder_path: Some("/tv".into()),
        series_type: None,
        monitor,
        monitored: true,
        season_folder: true,
        monitor_new_items: NewItemMonitor::All,
        use_scene_numbering: false,
        tags: vec![],
    }
}

async fn setup() -> (Db, FakeProvider, i32) {
    let db = Db::in_memory().await.unwrap();
    let profile = db
        .quality_profiles()
        .insert(&default_profiles().remove(0))
        .await
        .unwrap();
    (
        db,
        FakeProvider {
            info: Mutex::new(show()),
        },
        profile,
    )
}

#[tokio::test]
async fn adding_a_series_stores_every_episode_and_applies_the_monitor_mode() {
    let (db, provider, profile) = setup().await;
    let id = add_series(
        &db,
        &provider,
        &request(profile, MonitorMode::Future),
        now(),
    )
    .await
    .unwrap();

    let episodes = db.episodes().for_series(id).await.unwrap();
    assert_eq!(episodes.len(), 5);
    let monitored: Vec<_> = episodes
        .iter()
        .filter(|e| e.monitored)
        .map(|e| (e.season_number, e.episode_number))
        .collect();
    assert_eq!(
        monitored,
        [(2, 2)],
        "only the episode that has not aired yet"
    );

    // The season that holds it is monitored; the others are not.
    let series = db.series().get(id).await.unwrap();
    let flags: Vec<_> = series
        .seasons
        .iter()
        .map(|s| (s.season_number, s.monitored))
        .collect();
    assert_eq!(flags, [(0, false), (1, false), (2, true)]);
    assert_eq!(series.series_type, SeriesType::Standard);

    // Another mode can be applied later.
    apply_monitor_mode(&db, id, MonitorMode::All, now())
        .await
        .unwrap();
    let on: Vec<_> = db
        .episodes()
        .for_series(id)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| e.monitored)
        .map(|e| e.season_number)
        .collect();
    assert_eq!(on, [1, 1, 2, 2], "specials stay off");
    apply_monitor_mode(&db, id, MonitorMode::Skip, now())
        .await
        .unwrap();
    assert_eq!(
        db.episodes()
            .for_series(id)
            .await
            .unwrap()
            .iter()
            .filter(|e| e.monitored)
            .count(),
        4
    );
}

#[tokio::test]
async fn unknown_series_are_reported() {
    let (db, provider, profile) = setup().await;
    let mut req = request(profile, MonitorMode::All);
    req.tmdb_id = 999;
    assert!(add_series(&db, &provider, &req, now()).await.is_err());
    assert!(db.series().list().await.unwrap().is_empty());
}

#[tokio::test]
async fn refreshing_keeps_choices_and_follows_the_provider() {
    let (db, provider, profile) = setup().await;
    let id = add_series(
        &db,
        &provider,
        &request(profile, MonitorMode::Missing),
        now(),
    )
    .await
    .unwrap();
    let before = db.episodes().for_series(id).await.unwrap();
    let first = before
        .iter()
        .find(|e| e.season_number == 1 && e.episode_number == 1)
        .unwrap()
        .id;
    // The person unmonitors one episode and it has a file.
    db.episodes().set_monitored(&[first], false).await.unwrap();

    // The provider renames a show, retitles an episode, drops S2E2 and adds S3.
    let mut changed = show();
    changed.title = "Some Show Renamed".into();
    changed.status = SeriesStatus::Ended;
    changed
        .episodes
        .retain(|e| !(e.season_number == 2 && e.episode_number == 2));
    changed.episodes[1].title = "A better title".into();
    changed.episodes.push(episode(3, 1, 40));
    changed.seasons.push(SeasonInfo {
        season_number: 3,
        title: None,
        episode_count: 1,
        images: vec![],
    });
    provider.set(changed);

    let stats = refresh_series(&db, &provider, id).await.unwrap();
    assert_eq!((stats.added, stats.updated, stats.removed), (1, 4, 1));

    let series = db.series().get(id).await.unwrap();
    assert_eq!(series.info.title, "Some Show Renamed");
    assert_eq!(series.info.status, SeriesStatus::Ended);
    assert!(
        series.season(3).unwrap().monitored,
        "a new season follows 'monitor new items'"
    );

    let after = db.episodes().for_series(id).await.unwrap();
    assert_eq!(after.len(), 5);
    assert!(
        !after
            .iter()
            .any(|e| e.season_number == 2 && e.episode_number == 2)
    );
    let kept = after.iter().find(|e| e.id == first).unwrap();
    assert!(!kept.monitored, "the person's choice survives a refresh");
    assert_eq!(
        after
            .iter()
            .find(|e| e.season_number == 1 && e.episode_number == 1)
            .unwrap()
            .title,
        "A better title"
    );
    assert!(
        after
            .iter()
            .find(|e| e.season_number == 3)
            .unwrap()
            .monitored
    );
}

#[tokio::test]
async fn new_items_can_be_left_unmonitored() {
    let (db, provider, profile) = setup().await;
    let mut req = request(profile, MonitorMode::All);
    req.monitor_new_items = NewItemMonitor::None;
    let id = add_series(&db, &provider, &req, now()).await.unwrap();
    let mut changed = show();
    changed.episodes.push(episode(2, 3, 12));
    provider.set(changed);
    refresh_series(&db, &provider, id).await.unwrap();
    let added = db.episodes().find(id, 2, 3).await.unwrap().unwrap();
    assert!(!added.monitored);
}

#[tokio::test]
async fn anime_gets_absolute_numbers() {
    let (db, provider, profile) = setup().await;
    let mut req = request(profile, MonitorMode::All);
    req.series_type = Some(SeriesType::Anime);
    let id = add_series(&db, &provider, &req, now()).await.unwrap();
    let numbers: Vec<_> = db
        .episodes()
        .for_series(id)
        .await
        .unwrap()
        .into_iter()
        .map(|e| (e.season_number, e.episode_number, e.absolute_episode_number))
        .collect();
    assert_eq!(
        numbers,
        [
            (0, 1, None),
            (1, 1, Some(1)),
            (1, 2, Some(2)),
            (2, 1, Some(3)),
            (2, 2, Some(4))
        ]
    );
}
