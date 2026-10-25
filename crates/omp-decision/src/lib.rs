//! The release decision engine: given a release matched to a library movie,
//! should we grab it, and if several qualify, which is best?
//!
//! Ported from Radarr `DecisionEngine` (GPL-3.0). Everything the specs need
//! from the rest of the app comes through [`model::DecisionData`], so the
//! engine itself does no I/O.

pub mod compare;
pub mod engine;
pub mod model;
pub mod reason;
pub mod specs;
pub mod tv;

pub use engine::{Decision, DecisionEngine};
pub use model::{DecisionData, DecisionInfo, ReleaseSource, SearchContext, Subject};
pub use reason::{Rejection, RejectionKind, RejectionReason, Verdict};

#[cfg(test)]
mod spec_tests;
#[cfg(test)]
mod testkit;
