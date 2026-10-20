//! Metadata providers. The app talks to the [`MovieMetadataProvider`] trait;
//! providers are tried in order, so a hosted proxy can sit in front of TMDB
//! and TMDB can be the fallback (see PLAN.md, Q4/Q10).

pub mod tmdb;
mod tmdb_tv;

use async_trait::async_trait;
use omp_core::{movie::MovieMetadata, tv::SeriesInfo};

#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error("not found")]
    NotFound,
    #[error("the provider rejected our credentials")]
    Unauthorized,
    #[error("rate limited by the provider")]
    RateLimited,
    #[error("provider error: {0}")]
    Http(String),
    #[error("unexpected response: {0}")]
    Parse(String),
}

#[async_trait]
pub trait MovieMetadataProvider: Send + Sync {
    /// Full details for one movie.
    async fn movie(&self, tmdb_id: i64) -> Result<MovieMetadata, MetadataError>;

    /// Title search. Results carry summary data only (no release dates beyond
    /// the primary one, no credits); call [`Self::movie`] for details.
    async fn search(
        &self,
        term: &str,
        year: Option<i32>,
    ) -> Result<Vec<MovieMetadata>, MetadataError>;

    /// Looks a movie up by IMDb id ("tt0133093").
    async fn movie_by_imdb_id(&self, imdb_id: &str) -> Result<MovieMetadata, MetadataError>;
}

/// Series metadata, kept apart from movies so a provider can offer either.
#[async_trait]
pub trait SeriesMetadataProvider: Send + Sync {
    /// Full details for one series, with every season and episode.
    async fn series(&self, tmdb_id: i64) -> Result<SeriesInfo, MetadataError>;

    /// Title search. Results are summaries without episodes.
    async fn search_series(
        &self,
        term: &str,
        year: Option<i32>,
    ) -> Result<Vec<SeriesInfo>, MetadataError>;

    async fn series_by_imdb_id(&self, imdb_id: &str) -> Result<SeriesInfo, MetadataError>;

    async fn series_by_tvdb_id(&self, tvdb_id: i64) -> Result<SeriesInfo, MetadataError>;
}
