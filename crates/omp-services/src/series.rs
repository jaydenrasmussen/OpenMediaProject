//! Adding series, refreshing them from the metadata provider and choosing
//! which episodes to monitor. Ported from Sonarr `AddSeriesService`,
//! `RefreshEpisodeService` and `EpisodeMonitoredService` (GPL-3.0).

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use omp_core::tv::{
    EpisodeInfo, MonitorMode, NewItemMonitor, SeriesInfo, SeriesStatus, SeriesType,
};
use omp_db::{
    Db,
    repo::{EpisodeRecord, NewSeries, SeasonRecord},
};
use omp_metadata::SeriesMetadataProvider;

use crate::evaluate::ServiceError;

/// What to do when adding a series.
#[derive(Debug, Clone)]
pub struct AddSeries {
    pub tmdb_id: i64,
    pub quality_profile_id: i32,
    /// The series folder. The API builds it from the naming settings.
    pub path: String,
    pub root_folder_path: Option<String>,
    /// `None` takes the provider's suggestion.
    pub series_type: Option<SeriesType>,
    pub monitor: MonitorMode,
    pub monitored: bool,
    pub season_folder: bool,
    pub monitor_new_items: NewItemMonitor,
    pub use_scene_numbering: bool,
    pub tags: Vec<i32>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RefreshStats {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
}

/// Anime is numbered straight through, specials excepted.
fn absolute_numbers(episodes: &[EpisodeInfo]) -> HashMap<(i32, i32), i32> {
    let mut ordered: Vec<&EpisodeInfo> = episodes.iter().filter(|e| e.season_number > 0).collect();
    ordered.sort_by_key(|e| (e.season_number, e.episode_number));
    ordered
        .into_iter()
        .enumerate()
        .map(|(i, e)| ((e.season_number, e.episode_number), i as i32 + 1))
        .collect()
}

/// Episodes the provider lists twice (same season and number) count once.
fn dedupe(episodes: &[EpisodeInfo]) -> Vec<&EpisodeInfo> {
    let mut seen = HashSet::new();
    episodes
        .iter()
        .filter(|e| seen.insert((e.season_number, e.episode_number)))
        .collect()
}

/// A new episode starts monitored when its season is, except for the
/// "episode 0" extras that sit outside the first season.
fn starts_monitored(e: &EpisodeInfo, seasons: &[SeasonRecord]) -> bool {
    if e.episode_number == 0 && e.season_number != 1 {
        return false;
    }
    seasons
        .iter()
        .find(|s| s.season_number == e.season_number)
        .is_none_or(|s| s.monitored)
}

/// Which episodes a monitoring mode wants monitored, among all of a series'
/// episodes. Episodes not returned are unmonitored. `None` means "leave
/// things as they are".
pub fn monitored_by_mode(
    mode: MonitorMode,
    episodes: &[EpisodeRecord],
    now: DateTime<Utc>,
) -> Option<HashSet<i64>> {
    let first = episodes
        .iter()
        .map(|e| e.season_number)
        .filter(|s| *s > 0)
        .min()
        .unwrap_or(1);
    let last = episodes.iter().map(|e| e.season_number).max().unwrap_or(1);
    let pick = |wanted: &dyn Fn(&EpisodeRecord) -> bool| -> Option<HashSet<i64>> {
        Some(
            episodes
                .iter()
                .filter(|e| wanted(e))
                .map(|e| e.id)
                .collect(),
        )
    };
    let regular = |e: &EpisodeRecord| e.season_number > 0;
    match mode {
        MonitorMode::Skip => None,
        MonitorMode::None => Some(HashSet::new()),
        MonitorMode::All => pick(&regular),
        MonitorMode::Future => pick(&|e| regular(e) && e.air_date_utc.is_none_or(|d| d >= now)),
        MonitorMode::Missing => pick(&|e| regular(e) && !e.has_file()),
        MonitorMode::Existing => pick(&|e| regular(e) && e.has_file()),
        MonitorMode::Pilot => {
            pick(&|e| regular(e) && e.season_number == first && e.episode_number == 1)
        }
        MonitorMode::FirstSeason => pick(&|e| regular(e) && e.season_number == first),
        MonitorMode::LastSeason => pick(&|e| regular(e) && e.season_number == last),
        MonitorMode::Recent => {
            let cutoff = now - Duration::days(90);
            pick(&|e| regular(e) && e.air_date_utc.is_none_or(|d| d >= cutoff))
        }
        // These two only touch the specials and keep everything else.
        MonitorMode::MonitorSpecials | MonitorMode::UnmonitorSpecials => {
            let monitor_specials = mode == MonitorMode::MonitorSpecials;
            Some(
                episodes
                    .iter()
                    .filter(|e| {
                        if e.season_number == 0 {
                            monitor_specials
                        } else {
                            e.monitored
                        }
                    })
                    .map(|e| e.id)
                    .collect(),
            )
        }
    }
}

/// Sets episode and season monitoring from a mode.
pub async fn apply_monitor_mode(
    db: &Db,
    series_id: i64,
    mode: MonitorMode,
    now: DateTime<Utc>,
) -> Result<(), ServiceError> {
    let series = db.series().get(series_id).await?;
    let episodes = db.episodes().for_series(series_id).await?;
    let Some(wanted) = monitored_by_mode(mode, &episodes, now) else {
        return Ok(());
    };
    let (on, off): (Vec<i64>, Vec<i64>) = episodes
        .iter()
        .map(|e| e.id)
        .partition(|id| wanted.contains(id));
    db.episodes().set_monitored(&on, true).await?;
    db.episodes().set_monitored(&off, false).await?;

    let with_monitored: HashSet<i32> = episodes
        .iter()
        .filter(|e| wanted.contains(&e.id))
        .map(|e| e.season_number)
        .collect();
    let first = series
        .seasons
        .iter()
        .map(|s| s.season_number)
        .filter(|n| *n > 0)
        .min()
        .unwrap_or(1);
    let last = series
        .seasons
        .iter()
        .map(|s| s.season_number)
        .max()
        .unwrap_or(1);
    let still_going = matches!(
        series.info.status,
        SeriesStatus::Continuing | SeriesStatus::Upcoming
    );
    for season in &series.seasons {
        let n = season.season_number;
        // The newest season stays monitored when new episodes are expected.
        let monitored = if n > 0
            && n == last
            && (mode == MonitorMode::All || (mode == MonitorMode::Future && still_going))
        {
            true
        } else if n == first && mode == MonitorMode::Pilot {
            false
        } else {
            with_monitored.contains(&n)
        };
        db.series()
            .set_season_monitored(series_id, n, monitored)
            .await?;
    }
    Ok(())
}

/// Fetches a series and stores it with all its episodes.
pub async fn add_series(
    db: &Db,
    provider: &dyn SeriesMetadataProvider,
    req: &AddSeries,
    now: DateTime<Utc>,
) -> Result<i64, ServiceError> {
    let info = provider.series(req.tmdb_id).await?;
    let series_type = req.series_type.unwrap_or(info.suggested_type);
    let id = db
        .series()
        .add(&NewSeries {
            info: info.clone(),
            series_type,
            monitored: req.monitored,
            monitor_new_items: req.monitor_new_items,
            quality_profile_id: req.quality_profile_id,
            season_folder: req.season_folder,
            use_scene_numbering: req.use_scene_numbering,
            path: req.path.clone(),
            root_folder_path: req.root_folder_path.clone(),
            tags: req.tags.clone(),
        })
        .await?;
    // Seasons start monitored (specials do not); the mode refines this.
    let seasons: Vec<SeasonRecord> = info
        .seasons
        .iter()
        .map(|s| SeasonRecord {
            season_number: s.season_number,
            monitored: s.season_number > 0,
            title: s.title.clone(),
            images: s.images.clone(),
        })
        .collect();
    db.series().set_seasons(id, &seasons).await?;
    insert_episodes(db, id, series_type, &info, &seasons).await?;
    apply_monitor_mode(db, id, req.monitor, now).await?;
    Ok(id)
}

async fn insert_episodes(
    db: &Db,
    series_id: i64,
    series_type: SeriesType,
    info: &SeriesInfo,
    seasons: &[SeasonRecord],
) -> Result<usize, ServiceError> {
    let absolute = if series_type == SeriesType::Anime {
        absolute_numbers(&info.episodes)
    } else {
        HashMap::new()
    };
    let mut added = 0;
    for e in dedupe(&info.episodes) {
        db.episodes()
            .insert(
                series_id,
                e,
                absolute.get(&(e.season_number, e.episode_number)).copied(),
                starts_monitored(e, seasons),
            )
            .await?;
        added += 1;
    }
    Ok(added)
}

/// Re-reads a series from the provider: metadata, seasons and episodes. New
/// episodes follow the series' "monitor new items" choice; existing ones keep
/// their monitored flag and their file.
pub async fn refresh_series(
    db: &Db,
    provider: &dyn SeriesMetadataProvider,
    series_id: i64,
) -> Result<RefreshStats, ServiceError> {
    let series = db.series().get(series_id).await?;
    let info = provider.series(series.info.tmdb_id).await?;
    db.series().apply_info(series_id, &info).await?;

    // Seasons: existing ones keep their flag; new ones follow the series' choice.
    let new_monitored = series.monitor_new_items == NewItemMonitor::All;
    let seasons: Vec<SeasonRecord> = info
        .seasons
        .iter()
        .map(|s| SeasonRecord {
            season_number: s.season_number,
            monitored: series
                .season(s.season_number)
                .map_or(new_monitored && s.season_number > 0, |old| old.monitored),
            title: s.title.clone(),
            images: s.images.clone(),
        })
        .collect();
    db.series().set_seasons(series_id, &seasons).await?;

    let existing = db.episodes().for_series(series_id).await?;
    let mut by_key: HashMap<(i32, i32), &EpisodeRecord> = existing
        .iter()
        .map(|e| ((e.season_number, e.episode_number), e))
        .collect();
    let absolute = if series.series_type == SeriesType::Anime {
        absolute_numbers(&info.episodes)
    } else {
        HashMap::new()
    };

    let mut stats = RefreshStats::default();
    for e in dedupe(&info.episodes) {
        let abs = absolute.get(&(e.season_number, e.episode_number)).copied();
        match by_key.remove(&(e.season_number, e.episode_number)) {
            Some(old) => {
                db.episodes().apply_info(old.id, e, abs).await?;
                stats.updated += 1;
            }
            None => {
                db.episodes()
                    .insert(
                        series_id,
                        e,
                        abs,
                        new_monitored && starts_monitored(e, &seasons),
                    )
                    .await?;
                stats.added += 1;
            }
        }
    }
    // What the provider no longer lists goes away; files stay on disk and a
    // rescan will pick them up again if they still match something.
    let gone: Vec<i64> = by_key.values().map(|e| e.id).collect();
    stats.removed = gone.len();
    db.episodes().delete_many(&gone).await?;
    db.episode_files().delete_unused(series_id).await?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone};

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap()
    }

    fn ep(id: i64, season: i32, number: i32, days_from_now: i64, file: bool) -> EpisodeRecord {
        let when = now() + Duration::days(days_from_now);
        EpisodeRecord {
            id,
            series_id: 1,
            tmdb_id: id,
            season_number: season,
            episode_number: number,
            absolute_episode_number: None,
            scene_season_number: None,
            scene_episode_number: None,
            scene_absolute_episode_number: None,
            title: format!("E{number}"),
            overview: None,
            air_date: NaiveDate::from_ymd_opt(2026, 1, 1),
            air_date_utc: Some(when),
            runtime: 45,
            monitored: false,
            episode_file_id: file.then_some(1),
            last_search_time: None,
            finale_type: None,
        }
    }

    fn library() -> Vec<EpisodeRecord> {
        vec![
            ep(1, 0, 1, -900, false), // special
            ep(2, 1, 1, -400, true),  // old, have it
            ep(3, 1, 2, -399, false), // old, missing
            ep(4, 2, 1, -100, false), // recent-ish
            ep(5, 2, 2, -10, true),   // recent, have it
            ep(6, 2, 3, 20, false),   // future
        ]
    }

    fn ids(set: Option<HashSet<i64>>) -> Vec<i64> {
        let mut v: Vec<_> = set.unwrap().into_iter().collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn each_mode_picks_its_episodes() {
        let eps = library();
        let m = |mode| ids(monitored_by_mode(mode, &eps, now()));
        assert_eq!(m(MonitorMode::All), [2, 3, 4, 5, 6]);
        assert_eq!(m(MonitorMode::Future), [6]);
        assert_eq!(m(MonitorMode::Missing), [3, 4, 6]);
        assert_eq!(m(MonitorMode::Existing), [2, 5]);
        assert_eq!(m(MonitorMode::Pilot), [2]);
        assert_eq!(m(MonitorMode::FirstSeason), [2, 3]);
        assert_eq!(m(MonitorMode::LastSeason), [4, 5, 6]);
        assert_eq!(m(MonitorMode::Recent), [5, 6]);
        assert_eq!(m(MonitorMode::None), Vec::<i64>::new());
        assert!(monitored_by_mode(MonitorMode::Skip, &eps, now()).is_none());
    }

    #[test]
    fn special_modes_touch_only_season_zero() {
        let mut eps = library();
        for e in &mut eps {
            e.monitored = e.id == 3;
        }
        let m = |mode| ids(monitored_by_mode(mode, &eps, now()));
        assert_eq!(m(MonitorMode::MonitorSpecials), [1, 3]);
        assert_eq!(m(MonitorMode::UnmonitorSpecials), [3]);
    }

    #[test]
    fn absolute_numbers_run_straight_through_without_specials() {
        let mk = |s, n| EpisodeInfo {
            season_number: s,
            episode_number: n,
            ..Default::default()
        };
        let map = absolute_numbers(&[mk(0, 1), mk(1, 1), mk(1, 2), mk(2, 1)]);
        assert_eq!(map[&(1, 1)], 1);
        assert_eq!(map[&(1, 2)], 2);
        assert_eq!(map[&(2, 1)], 3);
        assert!(!map.contains_key(&(0, 1)));
    }

    #[test]
    fn episode_zero_outside_season_one_starts_unmonitored() {
        let seasons = vec![
            SeasonRecord {
                season_number: 1,
                monitored: true,
                title: None,
                images: vec![],
            },
            SeasonRecord {
                season_number: 2,
                monitored: true,
                title: None,
                images: vec![],
            },
            SeasonRecord {
                season_number: 3,
                monitored: false,
                title: None,
                images: vec![],
            },
        ];
        let e = |s, n| EpisodeInfo {
            season_number: s,
            episode_number: n,
            ..Default::default()
        };
        assert!(starts_monitored(&e(1, 0), &seasons));
        assert!(!starts_monitored(&e(2, 0), &seasons));
        assert!(starts_monitored(&e(2, 1), &seasons));
        assert!(!starts_monitored(&e(3, 1), &seasons));
        assert!(
            starts_monitored(&e(9, 1), &seasons),
            "a season we have no record of follows the default"
        );
    }
}
