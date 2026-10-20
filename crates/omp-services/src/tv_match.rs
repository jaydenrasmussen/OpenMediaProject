//! Matching a parsed release title to a library series and its episodes.
//! Ported from Sonarr `ParsingService` (GPL-3.0), without scene mappings.

use std::collections::HashMap;

use omp_db::{
    Db,
    repo::{EpisodeRecord, SeriesRecord},
};
use omp_parser::tv::{ParsedEpisodeInfo, clean_series_title};

use crate::evaluate::ServiceError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesMatchType {
    Title,
    Id,
}

/// The ids a release carries besides its title.
#[derive(Debug, Clone, Default)]
pub struct ReleaseIds {
    pub tmdb_id: i64,
    pub imdb_id: Option<String>,
}

/// Every library series, indexed for lookups by title. Loaded once per batch.
pub struct SeriesIndex {
    series: Vec<SeriesRecord>,
    episodes: tokio::sync::Mutex<HashMap<i64, Vec<EpisodeRecord>>>,
}

impl SeriesIndex {
    pub async fn load(db: &Db) -> Result<Self, ServiceError> {
        Ok(Self {
            series: db.series().list().await?,
            episodes: Default::default(),
        })
    }

    pub fn all(&self) -> &[SeriesRecord] {
        &self.series
    }

    fn titles(s: &SeriesRecord) -> Vec<String> {
        let mut t = vec![clean_series_title(&s.info.title)];
        if !s.info.original_title.is_empty() {
            t.push(clean_series_title(&s.info.original_title));
        }
        t.extend(
            s.info
                .alternative_titles
                .iter()
                .map(|a| clean_series_title(&a.title)),
        );
        t
    }

    /// Series whose title, original title or an alternative title cleans to `title`.
    /// The series' own titles win over alternative ones.
    pub fn by_title(&self, title: &str, year: Option<i32>) -> Option<&SeriesRecord> {
        let clean = clean_series_title(title);
        let matching: Vec<&SeriesRecord> = self
            .series
            .iter()
            .filter(|s| year.is_none_or(|y| s.info.year == y))
            .filter(|s| Self::titles(s).contains(&clean))
            .collect();
        matching
            .iter()
            .find(|s| {
                clean_series_title(&s.info.title) == clean
                    || clean_series_title(&s.info.original_title) == clean
            })
            .or(matching.first())
            .copied()
    }

    pub fn by_ids(&self, ids: &ReleaseIds) -> Option<&SeriesRecord> {
        self.series.iter().find(|s| {
            (ids.tmdb_id > 0 && s.info.tmdb_id == ids.tmdb_id)
                || ids
                    .imdb_id
                    .as_deref()
                    .is_some_and(|i| !i.is_empty() && s.info.imdb_id.as_deref() == Some(i))
        })
    }

    /// The leftmost, longest series title found inside a release title.
    pub fn inexact(&self, release_title: &str) -> Option<&SeriesRecord> {
        let clean = clean_series_title(release_title);
        self.series
            .iter()
            .filter_map(|s| {
                let own = clean_series_title(&s.info.title);
                clean
                    .find(&own)
                    .filter(|_| !own.is_empty())
                    .map(|p| (p, own.len(), s))
            })
            .min_by_key(|(pos, len, _)| (*pos, std::cmp::Reverse(*len)))
            .map(|(_, _, s)| s)
    }

    async fn episodes_of(
        &self,
        db: &Db,
        series_id: i64,
    ) -> Result<Vec<EpisodeRecord>, ServiceError> {
        let mut cache = self.episodes.lock().await;
        if let Some(e) = cache.get(&series_id) {
            return Ok(e.clone());
        }
        let e = db.episodes().for_series(series_id).await?;
        cache.insert(series_id, e.clone());
        Ok(e)
    }
}

/// The series a parsed title belongs to.
pub fn find_series<'a>(
    index: &'a SeriesIndex,
    parsed: &ParsedEpisodeInfo,
    ids: &ReleaseIds,
    target: Option<&'a SeriesRecord>,
) -> Option<(&'a SeriesRecord, SeriesMatchType)> {
    if let Some(t) = target {
        let clean = clean_series_title(&parsed.series_title);
        if SeriesIndex::titles(t).contains(&clean) {
            return Some((t, SeriesMatchType::Title));
        }
        if (ids.tmdb_id > 0 && ids.tmdb_id == t.info.tmdb_id)
            || ids
                .imdb_id
                .as_deref()
                .is_some_and(|i| t.info.imdb_id.as_deref() == Some(i))
        {
            return Some((t, SeriesMatchType::Id));
        }
    }
    let info = &parsed.series_title_info;
    if let Some(s) = index.by_title(&parsed.series_title, None) {
        return Some((s, SeriesMatchType::Title));
    }
    // Several titles in one release must all point at the same series.
    if info.all_titles.len() > 1 {
        let mut found: Option<&SeriesRecord> = None;
        for title in &info.all_titles {
            let hit = if info.year > 0 {
                index
                    .by_title(title, Some(info.year))
                    .or_else(|| index.by_title(&format!("{title} {}", info.year), None))
            } else {
                index.by_title(title, None)
            };
            match (hit, found) {
                (Some(h), Some(f)) if h.id != f.id => return None,
                (Some(h), None) => found = Some(h),
                _ => {}
            }
        }
        if let Some(f) = found {
            return Some((f, SeriesMatchType::Title));
        }
    }
    if info.year > 0
        && let Some(s) = index.by_title(&info.title_without_year, Some(info.year))
    {
        return Some((s, SeriesMatchType::Title));
    }
    index.by_ids(ids).map(|s| (s, SeriesMatchType::Id))
}

/// A release matched to its series and episodes.
pub struct TvMatch {
    pub series: SeriesRecord,
    pub match_type: SeriesMatchType,
    pub episodes: Vec<EpisodeRecord>,
    pub mapped_season: Option<i32>,
}

/// Which of `all` the parsed info names.
pub fn pick_episodes(
    parsed: &ParsedEpisodeInfo,
    all: &[EpisodeRecord],
    season: i32,
) -> Vec<EpisodeRecord> {
    let in_season = |n: i32| all.iter().filter(move |e| e.season_number == n).cloned();
    if parsed.full_season {
        if parsed.is_multi_season() {
            return parsed
                .season_numbers
                .iter()
                .flat_map(|n| in_season(*n))
                .collect();
        }
        return in_season(season).collect();
    }
    if parsed.is_daily() {
        let date = parsed.air_date.as_deref().unwrap_or_default();
        let mut same: Vec<&EpisodeRecord> = all
            .iter()
            .filter(|e| e.air_date.map(|d| d.to_string()).as_deref() == Some(date))
            .collect();
        same.sort_by_key(|e| e.episode_number);
        return match (same.len(), parsed.daily_part) {
            (1, _) => vec![same[0].clone()],
            (n, Some(part)) if n > 1 && part >= 1 => same
                .get(part as usize - 1)
                .map(|e| vec![(*e).clone()])
                .unwrap_or_default(),
            _ => vec![],
        };
    }
    if parsed.is_absolute_numbering() {
        let mut out = Vec::new();
        for abs in &parsed.absolute_episode_numbers {
            let hit = if parsed.special {
                all.iter()
                    .find(|e| e.season_number == 0 && e.episode_number == *abs)
            } else {
                all.iter().find(|e| e.absolute_episode_number == Some(*abs))
            };
            out.extend(hit.cloned());
        }
        return out;
    }
    if parsed.special && season != 0 {
        return vec![];
    }
    parsed
        .episode_numbers
        .iter()
        .filter_map(|n| {
            all.iter()
                .find(|e| e.season_number == season && e.episode_number == *n)
                .cloned()
        })
        .collect()
}

/// Matches a parsed release to the library. `None` when no series fits;
/// a series with no matching episodes comes back with an empty list.
pub async fn match_release(
    db: &Db,
    index: &SeriesIndex,
    parsed: &ParsedEpisodeInfo,
    ids: &ReleaseIds,
    target: Option<&SeriesRecord>,
) -> Result<Option<TvMatch>, ServiceError> {
    let Some((series, match_type)) = find_series(index, parsed, ids, target) else {
        return Ok(None);
    };
    let mapped_season = parsed.season_number();
    let episodes = match mapped_season {
        Some(season) => {
            // Daily shows only accept daily releases and the other way round.
            if parsed.is_daily() && series.series_type == omp_core::tv::SeriesType::Standard {
                vec![]
            } else {
                let all = index.episodes_of(db, series.id).await?;
                pick_episodes(parsed, &all, season)
            }
        }
        None => vec![],
    };
    Ok(Some(TvMatch {
        series: series.clone(),
        match_type,
        episodes,
        mapped_season,
    }))
}
