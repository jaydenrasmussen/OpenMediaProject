//! The user's library entries, as the decision engine sees them.

use chrono::{DateTime, Utc};

use crate::{movie::MovieStatus, quality::QualityModel};

/// A movie in the library (Radarr `Movie`, without the provider metadata).
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryMovie {
    pub id: i64,
    pub tmdb_id: i64,
    pub title: String,
    pub year: i32,
    pub monitored: bool,
    pub minimum_availability: MovieStatus,
    pub quality_profile_id: i32,
    pub path: String,
    pub tags: Vec<i32>,
}

/// The file we already have for a movie.
#[derive(Debug, Clone, PartialEq)]
pub struct ExistingFile {
    pub quality: QualityModel,
    pub size: u64,
    pub release_group: Option<String>,
    pub date_added: DateTime<Utc>,
    /// Ids of the custom formats this file matches, worked out by the caller.
    pub custom_format_ids: Vec<i32>,
}
