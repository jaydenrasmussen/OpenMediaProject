//! Radarr's metadata server (`api.radarr.video`): a hosted, cached front for
//! TMDB that needs no key. It is the primary movie source; [`Fallback`]
//! turns to TMDB when it is down or does not know a movie.
//!
//! The server is run by someone else and its answers are undocumented, so
//! parsing is tolerant and every failure is reported, never panicked on.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use omp_core::movie::{
    AlternativeTitle, CoverKind, Credit, CreditKind, MediaCover, MovieMetadata, Rating,
};
use serde::Deserialize;

use crate::{
    CollectionInfo, MetadataError, MovieMetadataProvider,
    tmdb::{CREW_JOBS, MAX_CAST, derive_status, parse_date},
};

const BASE: &str = "https://api.radarr.video/v1";

pub struct RadarrMetaProvider {
    http: reqwest::Client,
    base: String,
}

impl Default for RadarrMetaProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl RadarrMetaProvider {
    pub fn new() -> Self {
        Self {
            http: omp_net::client_builder()
                .timeout(Duration::from_secs(20))
                .build()
                .expect("static client config"),
            base: BASE.to_owned(),
        }
    }

    pub fn with_base_url(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, MetadataError> {
        let response = self
            .http
            .get(format!("{}{path}", self.base))
            .query(query)
            .send()
            .await
            .map_err(|e| MetadataError::Http(e.to_string()))?;
        match response.status().as_u16() {
            200 => response
                .json()
                .await
                .map_err(|e| MetadataError::Parse(e.to_string())),
            404 => Err(MetadataError::NotFound),
            429 => Err(MetadataError::RateLimited),
            other => Err(MetadataError::Http(format!("status {other} for {path}"))),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct RawMovie {
    tmdb_id: i64,
    imdb_id: Option<String>,
    title: String,
    original_title: String,
    original_language: String,
    overview: Option<String>,
    runtime: u32,
    popularity: f64,
    year: i32,
    in_cinema: Option<String>,
    physical_release: Option<String>,
    digital_release: Option<String>,
    images: Vec<RawImage>,
    ratings: Vec<RawRating>,
    genres: Vec<String>,
    alternative_titles: Vec<RawAlt>,
    credits: RawCredits,
    certifications: Vec<RawCertification>,
    studio: Option<String>,
    homepage: Option<String>,
    status: Option<String>,
    collection: Option<RawCollection>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct RawImage {
    cover_type: String,
    url: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct RawRating {
    count: u32,
    value: f64,
    origin: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct RawAlt {
    title: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct RawCredits {
    cast: Vec<RawPerson>,
    crew: Vec<RawPerson>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct RawPerson {
    name: String,
    order: u32,
    character: String,
    job: String,
    tmdb_id: i64,
    images: Option<Vec<RawImage>>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct RawCertification {
    country: String,
    certification: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct RawCollection {
    tmdb_id: i64,
    name: String,
    overview: Option<String>,
    parts: Option<Vec<RawMovie>>,
}

fn nonempty(s: Option<String>) -> Option<String> {
    s.filter(|s| !s.trim().is_empty())
}

fn date(s: &Option<String>) -> Option<DateTime<Utc>> {
    s.as_deref().and_then(parse_date)
}

fn covers(images: Vec<RawImage>) -> Vec<MediaCover> {
    images
        .into_iter()
        .filter(|i| !i.url.is_empty())
        .filter_map(|i| {
            let kind = match i.cover_type.as_str() {
                "Poster" => CoverKind::Poster,
                "Fanart" => CoverKind::Fanart,
                _ => return None,
            };
            Some(MediaCover { kind, url: i.url })
        })
        .collect()
}

fn map(raw: RawMovie, region: &str, now: DateTime<Utc>) -> MovieMetadata {
    let (in_cinemas, physical, digital) = (
        date(&raw.in_cinema),
        date(&raw.physical_release),
        date(&raw.digital_release),
    );
    let cast = raw
        .credits
        .cast
        .into_iter()
        .take(MAX_CAST)
        .map(|p| (CreditKind::Cast, p));
    let crew = raw
        .credits
        .crew
        .into_iter()
        .filter(|p| CREW_JOBS.contains(&p.job.as_str()))
        .map(|p| (CreditKind::Crew, p));
    let credits = cast
        .chain(crew)
        .map(|(kind, p)| Credit {
            person_tmdb_id: p.tmdb_id,
            name: p.name,
            kind,
            role: if kind == CreditKind::Cast {
                p.character
            } else {
                p.job
            },
            order: p.order,
            image_url: p.images.and_then(|i| i.into_iter().next()).map(|i| i.url),
        })
        .collect();
    let ratings = raw
        .ratings
        .iter()
        .find(|r| r.origin.eq_ignore_ascii_case("tmdb"))
        .or(raw.ratings.first())
        .filter(|r| r.count > 0)
        .map(|r| Rating {
            votes: r.count,
            value: r.value,
        });
    let (collection_tmdb_id, collection_title) = match raw.collection {
        Some(c) if c.tmdb_id > 0 => (Some(c.tmdb_id), Some(c.name)),
        _ => (None, None),
    };
    MovieMetadata {
        tmdb_id: raw.tmdb_id,
        imdb_id: nonempty(raw.imdb_id),
        title: raw.title,
        original_title: raw.original_title,
        original_language: raw.original_language,
        overview: nonempty(raw.overview),
        year: raw.year,
        runtime: raw.runtime,
        status: derive_status(
            raw.status.as_deref().unwrap_or_default(),
            in_cinemas,
            physical,
            digital,
            now,
        ),
        certification: raw
            .certifications
            .into_iter()
            .find(|c| c.country == region && !c.certification.is_empty())
            .map(|c| c.certification),
        genres: raw.genres,
        in_cinemas,
        physical_release: physical,
        digital_release: digital,
        collection_tmdb_id,
        collection_title,
        website: nonempty(raw.homepage),
        studio: nonempty(raw.studio),
        ratings,
        images: covers(raw.images),
        alternative_titles: raw
            .alternative_titles
            .into_iter()
            .filter(|a| !a.title.is_empty())
            .map(|a| AlternativeTitle {
                title: a.title,
                country: None,
            })
            .collect(),
        credits,
        popularity: raw.popularity,
        ..Default::default()
    }
}

#[async_trait]
impl MovieMetadataProvider for RadarrMetaProvider {
    async fn movie(&self, tmdb_id: i64) -> Result<MovieMetadata, MetadataError> {
        let raw: RawMovie = self.get(&format!("/movie/{tmdb_id}"), &[]).await?;
        if raw.tmdb_id == 0 {
            return Err(MetadataError::NotFound);
        }
        Ok(map(raw, "US", Utc::now()))
    }

    async fn search(
        &self,
        term: &str,
        year: Option<i32>,
    ) -> Result<Vec<MovieMetadata>, MetadataError> {
        let mut query = vec![("q", term.to_owned())];
        if let Some(y) = year {
            query.push(("year", y.to_string()));
        }
        let found: Vec<RawMovie> = self.get("/search", &query).await?;
        let now = Utc::now();
        Ok(found
            .into_iter()
            .filter(|r| r.tmdb_id > 0)
            .map(|r| map(r, "US", now))
            .collect())
    }

    async fn movie_by_imdb_id(&self, imdb_id: &str) -> Result<MovieMetadata, MetadataError> {
        let found: Vec<RawMovie> = self.get(&format!("/movie/imdb/{imdb_id}"), &[]).await?;
        found
            .into_iter()
            .find(|r| r.tmdb_id > 0)
            .map(|r| map(r, "US", Utc::now()))
            .ok_or(MetadataError::NotFound)
    }

    async fn collection(&self, tmdb_id: i64) -> Result<CollectionInfo, MetadataError> {
        let raw: RawCollection = self
            .get(&format!("/movie/collection/{tmdb_id}"), &[])
            .await?;
        let now = Utc::now();
        let mut movies: Vec<MovieMetadata> = raw
            .parts
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.tmdb_id > 0)
            .map(|r| map(r, "US", now))
            .collect();
        movies.sort_by_key(|m| (m.in_cinemas.is_none(), m.in_cinemas, m.year));
        Ok(CollectionInfo {
            tmdb_id: raw.tmdb_id,
            name: raw.name,
            overview: nonempty(raw.overview),
            movies,
        })
    }
}

/// Asks the primary provider first and the fallback when it fails or does
/// not know the movie.
pub struct Fallback {
    primary: Arc<dyn MovieMetadataProvider>,
    fallback: Arc<dyn MovieMetadataProvider>,
}

impl Fallback {
    pub fn new(
        primary: Arc<dyn MovieMetadataProvider>,
        fallback: Arc<dyn MovieMetadataProvider>,
    ) -> Self {
        Self { primary, fallback }
    }
}

/// The primary's answer if it gave one, else the fallback's. If both fail,
/// the fallback's error is the one to show.
fn either<T>(
    first: Result<T, MetadataError>,
    second: Result<T, MetadataError>,
) -> Result<T, MetadataError> {
    match (first, second) {
        (Ok(v), _) | (Err(_), Ok(v)) => Ok(v),
        (Err(_), Err(e)) => Err(e),
    }
}

#[async_trait]
impl MovieMetadataProvider for Fallback {
    async fn movie(&self, id: i64) -> Result<MovieMetadata, MetadataError> {
        match self.primary.movie(id).await {
            Ok(m) => Ok(m),
            Err(first) => either(Err(first), self.fallback.movie(id).await),
        }
    }

    async fn search(
        &self,
        term: &str,
        year: Option<i32>,
    ) -> Result<Vec<MovieMetadata>, MetadataError> {
        match self.primary.search(term, year).await {
            Ok(m) => Ok(m),
            Err(first) => either(Err(first), self.fallback.search(term, year).await),
        }
    }

    async fn movie_by_imdb_id(&self, imdb: &str) -> Result<MovieMetadata, MetadataError> {
        match self.primary.movie_by_imdb_id(imdb).await {
            Ok(m) => Ok(m),
            Err(first) => either(Err(first), self.fallback.movie_by_imdb_id(imdb).await),
        }
    }

    async fn collection(&self, id: i64) -> Result<CollectionInfo, MetadataError> {
        match self.primary.collection(id).await {
            Ok(c) if !c.movies.is_empty() => Ok(c),
            first => either(first, self.fallback.collection(id).await),
        }
    }
}
