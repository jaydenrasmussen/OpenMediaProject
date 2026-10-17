//! One small struct per table group, borrowed from [`crate::Db`].

mod config;
mod profiles;
mod simple;

pub use config::Config;
pub use profiles::{
    CustomFormats, DelayProfiles, QualityDefinitionsRepo, QualityProfiles, ReleaseProfiles,
};
pub use simple::{RootFolders, Tags};
