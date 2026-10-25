//! The decision engine for TV releases: should a release for some episodes
//! be grabbed, and which of several is best? Ported from Sonarr (GPL-3.0).

pub mod compare;
pub mod engine;
pub mod model;
pub mod specs;

pub use engine::TvDecisionEngine;
pub use model::{
    EpisodeFileView, EpisodeSubject, EpisodeView, SeriesView, TvDecisionData, TvDecisionInfo,
    TvQueueEntry, TvSearchContext, TvSearchKind,
};

#[cfg(test)]
mod tests;
