//! TV domain types: what a metadata provider says about a series, and the
//! choices a person makes about how to follow it.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::movie::{AlternativeTitle, MediaCover, Rating};

/// How episodes are numbered and found (Sonarr `SeriesTypes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeriesType {
    /// Season and episode numbers.
    #[default]
    Standard,
    /// Named by air date.
    Daily,
    /// Absolute episode numbers, often with a fansub group in front.
    Anime,
}

impl SeriesType {
    pub fn as_i32(self) -> i32 {
        match self {
            SeriesType::Standard => 0,
            SeriesType::Daily => 1,
            SeriesType::Anime => 2,
        }
    }

    pub fn from_i32(n: i32) -> Self {
        match n {
            1 => SeriesType::Daily,
            2 => SeriesType::Anime,
            _ => SeriesType::Standard,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "standard" => Some(SeriesType::Standard),
            "daily" => Some(SeriesType::Daily),
            "anime" => Some(SeriesType::Anime),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SeriesStatus {
    #[default]
    Continuing,
    Ended,
    Upcoming,
    Deleted,
}

impl SeriesStatus {
    pub fn as_i32(self) -> i32 {
        match self {
            SeriesStatus::Deleted => -1,
            SeriesStatus::Continuing => 0,
            SeriesStatus::Ended => 1,
            SeriesStatus::Upcoming => 2,
        }
    }

    pub fn from_i32(n: i32) -> Self {
        match n {
            -1 => SeriesStatus::Deleted,
            1 => SeriesStatus::Ended,
            2 => SeriesStatus::Upcoming,
            _ => SeriesStatus::Continuing,
        }
    }
}

/// Which episodes to monitor when a series is added (Sonarr `MonitorTypes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorMode {
    #[default]
    All,
    /// Episodes that have not aired yet.
    Future,
    /// Episodes without a file.
    Missing,
    /// Episodes that already have a file.
    Existing,
    FirstSeason,
    LastSeason,
    /// Only the first episode.
    Pilot,
    /// Aired in the last 90 days, and future ones.
    Recent,
    MonitorSpecials,
    UnmonitorSpecials,
    None,
    /// Leave episodes as they are.
    Skip,
}

/// Whether episodes that appear later are monitored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NewItemMonitor {
    #[default]
    All,
    None,
}

/// One season as the provider describes it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SeasonInfo {
    pub season_number: i32,
    pub title: Option<String>,
    pub episode_count: u32,
    pub images: Vec<MediaCover>,
}

/// One episode as the provider describes it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EpisodeInfo {
    pub tmdb_id: i64,
    pub season_number: i32,
    pub episode_number: i32,
    pub title: String,
    pub overview: Option<String>,
    pub air_date: Option<NaiveDate>,
    pub air_date_utc: Option<DateTime<Utc>>,
    /// Minutes; 0 when unknown.
    pub runtime: u32,
    /// "series", "season" or "midseason" when the provider says so.
    pub finale_type: Option<String>,
}

/// What a metadata provider knows about a series. `episodes` is empty in
/// search results and filled by a full lookup.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SeriesInfo {
    pub tmdb_id: i64,
    pub tvdb_id: Option<i64>,
    pub imdb_id: Option<String>,
    pub title: String,
    pub original_title: String,
    /// ISO 639-1 code, as the provider reports it.
    pub original_language: String,
    pub overview: Option<String>,
    pub year: i32,
    pub status: SeriesStatus,
    pub network: Option<String>,
    /// Typical episode length in minutes; 0 when unknown.
    pub runtime: u32,
    pub genres: Vec<String>,
    pub certification: Option<String>,
    pub first_aired: Option<NaiveDate>,
    pub last_aired: Option<NaiveDate>,
    pub images: Vec<MediaCover>,
    pub alternative_titles: Vec<AlternativeTitle>,
    pub ratings: Option<Rating>,
    /// The type the provider's data suggests (talk shows are daily, anime is anime).
    pub suggested_type: SeriesType,
    pub seasons: Vec<SeasonInfo>,
    pub episodes: Vec<EpisodeInfo>,
}
