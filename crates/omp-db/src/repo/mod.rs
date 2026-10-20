//! One small struct per table group, borrowed from [`crate::Db`].

mod activity;
mod auth;
mod config;
mod movies;
mod profiles;
mod providers;
mod simple;

pub use activity::{
    Blocklist, History, HistoryRecord, PendingReason, PendingRelease, PendingReleases,
};
pub use auth::{Sessions, User, Users};
pub use config::Config;
pub use movies::{MovieFileRecord, MovieFiles, MovieRecord, MovieUpdate, Movies, NewMovie};
pub use profiles::{
    CustomFormats, DelayProfiles, QualityDefinitionsRepo, QualityProfiles, ReleaseProfiles,
};
pub use providers::{Provider, ProviderKind, Providers};
pub use simple::{ImportExclusion, ImportExclusions, RootFolder, RootFolders, Tag, Tags};
