//! One small struct per table group, borrowed from [`crate::Db`].

mod activity;
mod auth;
mod config;
mod logs;
mod media_servers;
mod movies;
mod profiles;
mod providers;
mod scene;
mod series;
mod simple;
mod tv_activity;

pub use activity::{
    Blocklist, History, HistoryRecord, PendingReason, PendingRelease, PendingReleases,
};
pub use auth::{Sessions, User, Users};
pub use config::Config;
pub use logs::{LogEntry, Logs};
pub use media_servers::{MediaItem, MediaServerItems, SyncStatus};
pub use movies::{MovieFileRecord, MovieFiles, MovieRecord, MovieUpdate, Movies, NewMovie};
pub use profiles::{
    CustomFormats, DelayProfiles, QualityDefinitionsRepo, QualityProfiles, ReleaseProfiles,
};
pub use providers::{Provider, ProviderKind, Providers};
pub use scene::{SceneTitleRow, SceneTitles};
pub use series::{
    EpisodeFileRecord, EpisodeFiles, EpisodeRecord, Episodes, NewSeries, SeasonRecord, Series,
    SeriesRecord, SeriesUpdate,
};
pub use simple::{
    ImportExclusion, ImportExclusions, RemotePathMapping, RemotePathMappings, RootFolder,
    RootFolders, SeriesExclusions, Tag, Tags,
};
pub use tv_activity::{
    EpisodeHistory, EpisodeHistoryRecord, PendingTvRelease, PendingTvReleases, TvBlocklist,
    TvBlocklistEntry,
};
