//! What the engine looks at (`Subject`) and what it asks the rest of the app
//! (`DecisionData`).

use chrono::{DateTime, Utc};
use omp_core::{
    language::Language,
    library::{ExistingFile, LibraryMovie},
    movie::MovieMetadata,
    quality::QualityModel,
    release::{IndexerFlags, ReleaseInfo},
};
use omp_parser::title::ParsedMovieInfo;
use omp_quality::{
    definition::QualityDefinitions, delay::DelayProfile, profile::QualityProfile,
    release_profile::ReleaseProfile, upgrade::ProperDownloadTypes,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseSource {
    Rss,
    Search,
    UserInvokedSearch,
    InteractiveSearch,
    ReleasePush,
}

/// A release matched to a library movie, with everything derived from it
/// (Radarr `RemoteMovie`).
pub struct Subject<'a> {
    pub release: &'a ReleaseInfo,
    pub parsed: &'a ParsedMovieInfo,
    pub movie: &'a LibraryMovie,
    pub metadata: &'a MovieMetadata,
    pub profile: &'a QualityProfile,
    /// The file we have today, if any.
    pub existing_file: Option<&'a ExistingFile>,
    /// Custom formats this release matched, and their summed score.
    pub custom_format_ids: &'a [i32],
    pub custom_format_score: i32,
    pub languages: &'a [Language],
    pub source: ReleaseSource,
}

/// A targeted search the user or scheduler started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchContext {
    pub movie_id: i64,
    pub user_invoked: bool,
    pub interactive: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DecisionInfo {
    pub pushed_release: bool,
    pub search: Option<SearchContext>,
}

impl DecisionInfo {
    pub fn user_invoked_search(&self) -> bool {
        self.search.is_some_and(|s| s.user_invoked)
    }
}

/// Settings the specs read. Stored as JSON; missing fields take defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DecisionConfig {
    pub propers_and_repacks: ProperDownloadTypes,
    /// Minutes a usenet release must age before it is grabbed. 0 disables.
    pub minimum_age: i32,
    /// Days of usenet retention. 0 disables.
    pub retention: i32,
    /// Megabytes; 0 disables.
    pub maximum_size: i64,
    pub enable_completed_download_handling: bool,
    pub skip_free_space_check_when_importing: bool,
    /// Megabytes.
    pub minimum_free_space_when_importing: i64,
    pub allow_hardcoded_subs: bool,
    /// Comma separated.
    pub whitelisted_hardcoded_subs: String,
    /// Days after the availability date before a movie is searched for.
    pub availability_delay: i64,
    pub prefer_indexer_flags: bool,
}

impl Default for DecisionConfig {
    fn default() -> Self {
        Self {
            propers_and_repacks: ProperDownloadTypes::PreferAndUpgrade,
            minimum_age: 0,
            retention: 0,
            maximum_size: 0,
            enable_completed_download_handling: true,
            skip_free_space_check_when_importing: false,
            minimum_free_space_when_importing: 100,
            allow_hardcoded_subs: false,
            whitelisted_hardcoded_subs: String::new(),
            availability_delay: 0,
            prefer_indexer_flags: false,
        }
    }
}

/// What the library knows about an indexer.
#[derive(Debug, Clone, Default)]
pub struct IndexerInfo {
    pub id: i32,
    pub tags: Vec<i32>,
    /// Present for torrent indexers.
    pub torrent: Option<TorrentIndexerSettings>,
    /// TV: when a season search finds single episodes of a season whose last
    /// episode aired more than this many days ago, ask for the season pack
    /// instead. 0 turns the rule off.
    pub season_pack_max_single_episode_age_days: u32,
}

#[derive(Debug, Clone, Default)]
pub struct TorrentIndexerSettings {
    pub minimum_seeders: i32,
    /// A release needs at least one of these; empty means no requirement.
    pub required_flags: Vec<IndexerFlags>,
}

pub use omp_core::history::HistoryEvent;

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub event: HistoryEvent,
    pub date: DateTime<Utc>,
    pub quality: QualityModel,
    pub custom_format_ids: Vec<i32>,
    /// Torrent info hash (upper case) or the download client's id.
    pub download_id: Option<String>,
    pub source_title: String,
}

#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub quality: QualityModel,
    pub custom_format_ids: Vec<i32>,
    /// Failed downloads waiting to be retried do not block new grabs.
    pub failed_pending: bool,
}

/// Everything the specs need that is not in the [`Subject`]. The app backs
/// this with the database; tests use a fake.
pub trait DecisionData {
    fn now(&self) -> DateTime<Utc>;
    fn config(&self) -> &DecisionConfig;
    fn quality_definitions(&self) -> &QualityDefinitions;
    fn delay_profile_for(&self, tags: &[i32]) -> &DelayProfile;
    fn is_blocklisted(&self, movie_id: i64, release: &ReleaseInfo) -> bool;
    /// When the indexer is blocked until, if it is blocked now.
    fn indexer_blocked_till(&self, indexer_id: i32) -> Option<DateTime<Utc>>;
    fn indexer(&self, indexer_id: i32) -> Option<IndexerInfo>;
    fn release_profiles(&self, tags: &[i32], indexer_id: i32) -> Vec<ReleaseProfile>;
    /// Newest first.
    fn history_for_movie(&self, movie_id: i64) -> Vec<HistoryEntry>;
    fn queue_for_movie(&self, movie_id: i64) -> Vec<QueueEntry>;
    /// Movies that have a release waiting in the pending queue.
    fn movie_has_pending_release(&self, movie_id: i64) -> bool;
    /// Age in minutes of the oldest pending release for the movie.
    fn oldest_pending_release_age_minutes(&self, movie_id: i64) -> Option<f64>;
    /// Free bytes on the volume holding `path`; `None` when unknown.
    fn free_space(&self, path: &str) -> Option<u64>;
}
