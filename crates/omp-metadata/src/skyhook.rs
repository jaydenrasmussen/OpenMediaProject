//! Sonarr's metadata proxy (SkyHook), a caching front for TVDB. Its episode
//! list uses TVDB numbering and air times, which is what indexers, scene
//! releases and TheXEM go by, so it is preferred for episodes. Everything
//! else comes from the provider it wraps (TMDB), which is also the fallback
//! when SkyHook is down or does not know the series.
//!
//! SkyHook is volunteer-run and its responses are undocumented, so parsing
//! is tolerant and every failure falls back quietly (see PLAN.md, Q10).

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use omp_core::tv::{EpisodeInfo, SeasonInfo, SeriesInfo, SeriesStatus};
use serde::Deserialize;

use crate::{MetadataError, SeriesMetadataProvider};

const BASE: &str = "https://skyhook.sonarr.tv/v1/tvdb";

pub struct SkyhookSeriesProvider {
    http: reqwest::Client,
    base: String,
    inner: Arc<dyn SeriesMetadataProvider>,
}

impl SkyhookSeriesProvider {
    /// `inner` supplies everything SkyHook does not and is the fallback.
    pub fn new(inner: Arc<dyn SeriesMetadataProvider>) -> Self {
        Self {
            http: omp_net::client_builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("static client config"),
            base: BASE.to_owned(),
            inner,
        }
    }

    pub fn with_base_url(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    async fn show(&self, tvdb_id: i64) -> Result<RawShow, MetadataError> {
        let response = self
            .http
            .get(format!("{}/shows/en/{tvdb_id}", self.base))
            .send()
            .await
            .map_err(|e| MetadataError::Http(e.to_string()))?;
        match response.status().as_u16() {
            200 => response
                .json()
                .await
                .map_err(|e| MetadataError::Parse(e.to_string())),
            404 => Err(MetadataError::NotFound),
            429 => Err(MetadataError::RateLimited),
            other => Err(MetadataError::Http(format!("HTTP {other}"))),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawShow {
    #[serde(default, rename = "tmdbId")]
    tmdb_id: Option<i64>,
    #[serde(default)]
    status: String,
    #[serde(default)]
    runtime: u32,
    #[serde(default, rename = "alternativeTitles")]
    alternative_titles: Vec<RawAlt>,
    #[serde(default)]
    seasons: Vec<RawSeason>,
    #[serde(default)]
    episodes: Vec<RawEpisode>,
}

#[derive(Debug, Deserialize)]
struct RawAlt {
    title: String,
}

#[derive(Debug, Deserialize)]
struct RawSeason {
    #[serde(rename = "seasonNumber")]
    season_number: i32,
    #[serde(default)]
    images: Vec<RawImage>,
}

#[derive(Debug, Deserialize)]
struct RawImage {
    #[serde(rename = "coverType")]
    cover_type: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct RawEpisode {
    #[serde(default, rename = "tvdbId")]
    tvdb_id: i64,
    #[serde(rename = "seasonNumber")]
    season_number: i32,
    #[serde(rename = "episodeNumber")]
    episode_number: i32,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default, rename = "airDate")]
    air_date: Option<String>,
    #[serde(default, rename = "airDateUtc")]
    air_date_utc: Option<String>,
    #[serde(default)]
    runtime: Option<u32>,
    #[serde(default, rename = "finaleType")]
    finale_type: Option<String>,
}

fn status(s: &str) -> Option<SeriesStatus> {
    match s.to_ascii_lowercase().as_str() {
        "continuing" => Some(SeriesStatus::Continuing),
        "ended" => Some(SeriesStatus::Ended),
        "upcoming" => Some(SeriesStatus::Upcoming),
        "deleted" => Some(SeriesStatus::Deleted),
        _ => None,
    }
}

fn map_episode(e: RawEpisode) -> EpisodeInfo {
    EpisodeInfo {
        tmdb_id: e.tvdb_id,
        season_number: e.season_number,
        episode_number: e.episode_number,
        title: e
            .title
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| "TBA".into()),
        overview: e.overview.filter(|o| !o.trim().is_empty()),
        air_date: e
            .air_date
            .as_deref()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
        air_date_utc: e
            .air_date_utc
            .as_deref()
            .and_then(|d| DateTime::parse_from_rfc3339(d).ok())
            .map(|d| d.with_timezone(&Utc)),
        runtime: e.runtime.unwrap_or(0),
        finale_type: e.finale_type.filter(|f| !f.is_empty()),
    }
}

/// `base` (from the wrapped provider) with SkyHook's episodes, seasons,
/// status and titles laid over it.
fn enrich(mut base: SeriesInfo, show: RawShow) -> SeriesInfo {
    let episodes: Vec<EpisodeInfo> = show.episodes.into_iter().map(map_episode).collect();
    let mut counts: BTreeMap<i32, u32> = BTreeMap::new();
    for e in &episodes {
        *counts.entry(e.season_number).or_default() += 1;
    }
    let mut seasons = Vec::new();
    for (number, count) in counts {
        let old = base.seasons.iter().find(|s| s.season_number == number);
        let sky = show.seasons.iter().find(|s| s.season_number == number);
        let images = match sky {
            Some(s) if !s.images.is_empty() => s
                .images
                .iter()
                .filter_map(|i| {
                    let kind = match i.cover_type.as_str() {
                        "Poster" => omp_core::movie::CoverKind::Poster,
                        "Fanart" => omp_core::movie::CoverKind::Fanart,
                        _ => return None,
                    };
                    Some(omp_core::movie::MediaCover {
                        kind,
                        url: i.url.clone(),
                    })
                })
                .collect(),
            _ => old.map(|s| s.images.clone()).unwrap_or_default(),
        };
        seasons.push(SeasonInfo {
            season_number: number,
            title: old.and_then(|s| s.title.clone()),
            episode_count: count,
            images,
        });
    }
    base.episodes = episodes;
    base.seasons = seasons;
    if let Some(s) = status(&show.status) {
        base.status = s;
    }
    if show.runtime > 0 {
        base.runtime = show.runtime;
    }
    for alt in show.alternative_titles {
        if !alt.title.trim().is_empty()
            && alt.title != base.title
            && !base.alternative_titles.iter().any(|a| a.title == alt.title)
        {
            base.alternative_titles
                .push(omp_core::movie::AlternativeTitle {
                    title: alt.title,
                    country: None,
                });
        }
    }
    base
}

#[async_trait]
impl SeriesMetadataProvider for SkyhookSeriesProvider {
    async fn series(&self, tmdb_id: i64) -> Result<SeriesInfo, MetadataError> {
        let base = self.inner.series(tmdb_id).await?;
        let Some(tvdb) = base.tvdb_id else {
            return Ok(base);
        };
        match self.show(tvdb).await {
            // A mismatch means TVDB and TMDB disagree about what this is.
            Ok(show)
                if show.tmdb_id.is_none_or(|t| t == 0 || t == tmdb_id)
                    && !show.episodes.is_empty() =>
            {
                Ok(enrich(base, show))
            }
            Ok(_) => Ok(base),
            Err(e) => {
                tracing::debug!(
                    tvdb,
                    "SkyHook unavailable, using the fallback's episodes: {e}"
                );
                Ok(base)
            }
        }
    }

    async fn search_series(
        &self,
        term: &str,
        year: Option<i32>,
    ) -> Result<Vec<SeriesInfo>, MetadataError> {
        self.inner.search_series(term, year).await
    }

    async fn series_by_imdb_id(&self, imdb_id: &str) -> Result<SeriesInfo, MetadataError> {
        self.inner.series_by_imdb_id(imdb_id).await
    }

    async fn series_by_tvdb_id(&self, tvdb_id: i64) -> Result<SeriesInfo, MetadataError> {
        self.inner.series_by_tvdb_id(tvdb_id).await
    }
}
