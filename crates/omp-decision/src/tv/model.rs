//! What the TV specs look at and what they ask the rest of the app.

use chrono::{DateTime, Utc};
use omp_core::{quality::QualityModel, release::ReleaseInfo, tv::SeriesType};
use omp_parser::tv::ParsedEpisodeInfo;
use omp_quality::{
    definition::QualityDefinitions, delay::DelayProfile, profile::QualityProfile,
    release_profile::ReleaseProfile,
};

use crate::model::{DecisionConfig, HistoryEntry, IndexerInfo, ReleaseSource};

/// The series fields the specs use.
#[derive(Debug, Clone)]
pub struct SeriesView {
    pub id: i64,
    pub title: String,
    pub series_type: SeriesType,
    pub monitored: bool,
    pub tags: Vec<i32>,
    /// Typical episode length in minutes; 0 when unknown.
    pub runtime: u32,
    pub path: String,
    /// Season number of the first regular season, and when its first
    /// episode aired. Used to guess a runtime for brand-new series.
    pub first_season: Option<i32>,
    pub first_season_start: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct EpisodeView {
    pub id: i64,
    pub season_number: i32,
    pub episode_number: i32,
    pub runtime: u32,
    pub air_date_utc: Option<DateTime<Utc>>,
    pub monitored: bool,
    pub file_id: Option<i64>,
}

/// A file we already have, with every episode it covers.
#[derive(Debug, Clone)]
pub struct EpisodeFileView {
    pub id: i64,
    pub quality: QualityModel,
    pub release_group: Option<String>,
    pub date_added: DateTime<Utc>,
    pub custom_format_ids: Vec<i32>,
    /// All episodes this file holds (a file can span several).
    pub episode_ids: Vec<i64>,
}

/// A release matched to a series and its episodes (Sonarr `RemoteEpisode`).
pub struct EpisodeSubject<'a> {
    pub release: &'a ReleaseInfo,
    pub parsed: &'a ParsedEpisodeInfo,
    pub series: &'a SeriesView,
    pub episodes: &'a [EpisodeView],
    /// Files of the matched episodes, each once.
    pub files: &'a [EpisodeFileView],
    pub profile: &'a QualityProfile,
    pub custom_format_ids: &'a [i32],
    pub custom_format_score: i32,
    pub source: ReleaseSource,
    /// The season number after scene mapping; plain parsing gives the parsed one.
    pub mapped_season: Option<i32>,
}

impl EpisodeSubject<'_> {
    /// The file holding an episode, if it has one.
    pub fn file_of(&self, e: &EpisodeView) -> Option<&EpisodeFileView> {
        e.file_id
            .and_then(|id| self.files.iter().find(|f| f.id == id))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TvSearchKind {
    SingleEpisode { season: i32, episode: i32 },
    Season { season: i32 },
    Daily,
    AnimeEpisode { is_season_search: bool },
    AnimeSeason,
    Special,
}

/// A targeted search a person or the scheduler started.
#[derive(Debug, Clone)]
pub struct TvSearchContext {
    pub series_id: i64,
    /// The episodes the search is for.
    pub episode_ids: Vec<i64>,
    /// Season and air time of each episode searched for.
    pub episodes: Vec<(i32, Option<DateTime<Utc>>)>,
    pub kind: TvSearchKind,
    pub user_invoked: bool,
    pub interactive: bool,
    /// Only monitored episodes may be grabbed (an interactive pick may ignore this).
    pub monitored_only: bool,
}

#[derive(Debug, Clone, Default)]
pub struct TvDecisionInfo {
    pub pushed_release: bool,
    pub search: Option<TvSearchContext>,
}

impl TvDecisionInfo {
    pub fn user_invoked_search(&self) -> bool {
        self.search.as_ref().is_some_and(|s| s.user_invoked)
    }
}

/// An item in the download queue that covers some episodes.
#[derive(Debug, Clone)]
pub struct TvQueueEntry {
    pub episode_ids: Vec<i64>,
    pub quality: QualityModel,
    pub custom_format_ids: Vec<i32>,
    pub failed_pending: bool,
}

/// What the specs need beyond the subject. The app backs this with the
/// database; tests use a fake.
pub trait TvDecisionData {
    fn now(&self) -> DateTime<Utc>;
    fn config(&self) -> &DecisionConfig;
    fn quality_definitions(&self) -> &QualityDefinitions;
    fn delay_profile_for(&self, tags: &[i32]) -> &DelayProfile;
    fn is_blocklisted(&self, series_id: i64, release: &ReleaseInfo) -> bool;
    fn indexer_blocked_till(&self, indexer_id: i32) -> Option<DateTime<Utc>>;
    fn indexer(&self, indexer_id: i32) -> Option<IndexerInfo>;
    fn release_profiles(&self, tags: &[i32], indexer_id: i32) -> Vec<ReleaseProfile>;
    /// Newest first.
    fn history_for_episode(&self, episode_id: i64) -> Vec<HistoryEntry>;
    fn queue_for_series(&self, series_id: i64) -> Vec<TvQueueEntry>;
    /// Does a pending release already cover any of these episodes?
    fn has_pending_release(&self, series_id: i64, episode_ids: &[i64]) -> bool;
    /// Age in minutes of the oldest pending release covering any of the episodes.
    fn oldest_pending_age_minutes(&self, series_id: i64, episode_ids: &[i64]) -> Option<f64>;
    fn free_space(&self, path: &str) -> Option<u64>;
}
