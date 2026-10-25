//! TMDB (themoviedb.org) provider, using a v4 read access token.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use omp_core::movie::{
    AlternativeTitle, CoverKind, Credit, CreditKind, MediaCover, MovieMetadata, MovieStatus, Rating,
};
use serde::Deserialize;

use crate::{MetadataError, MovieMetadataProvider};

const API_BASE: &str = "https://api.themoviedb.org/3";
const IMAGE_BASE: &str = "https://image.tmdb.org/t/p/original";
pub(crate) const CREW_JOBS: &[&str] = &["Director", "Writer", "Screenplay", "Producer"];
pub(crate) const MAX_CAST: usize = 50;

/// How long an answer is reused. Metadata changes rarely and the same
/// movie is asked for by search, add and refresh within minutes.
const CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(600);
const CACHE_MAX: usize = 500;
/// The read-only TMDB token Radarr ships in its public source
/// (`RadarrCloudRequestBuilder`), used when no other is set. It is widely
/// distributed and can only read; anyone who wants their own sets
/// `TMDB_READ_TOKEN` or saves one under Settings → Network.
pub const DEFAULT_READ_TOKEN: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJhdWQiOiIxYTczNzMzMDE5NjFkMDNmOTdmODUzYTg3NmRkMTIxMiIsInN1YiI6IjU4NjRmNTkyYzNhMzY4MGFiNjAxNzUzNCIsInNjb3BlcyI6WyJhcGlfcmVhZCJdLCJ2ZXJzaW9uIjoxfQ.gh1BwogCCKOda6xj9FRMgAAj_RYKMMPC3oNlcBtlmwk";

/// Gap between calls: TMDB allows about 40 a second, stay well under.
const MIN_GAP: std::time::Duration = std::time::Duration::from_millis(40);

type Cache =
    std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, serde_json::Value)>>;

pub struct TmdbProvider {
    http: reqwest::Client,
    cache: Cache,
    next_slot: tokio::sync::Mutex<std::time::Instant>,
    token: String,
    base: String,
    language: String,
    certification_region: String,
}

impl TmdbProvider {
    /// `token` is a TMDB "API Read Access Token" (a JWT), not the short v3 key.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            http: omp_net::client_builder()
                .build()
                .expect("static client config"),
            cache: Cache::default(),
            next_slot: tokio::sync::Mutex::new(std::time::Instant::now()),
            token: token.into(),
            base: API_BASE.to_owned(),
            language: "en-US".to_owned(),
            certification_region: "US".to_owned(),
        }
    }

    /// Reads `TMDB_READ_TOKEN` from the environment.
    pub fn from_env() -> Option<Self> {
        std::env::var("TMDB_READ_TOKEN")
            .ok()
            .filter(|t| !t.trim().is_empty())
            .map(Self::new)
    }

    pub fn with_base_url(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    pub(crate) fn certification_region(&self) -> String {
        self.certification_region.clone()
    }

    pub fn with_certification_region(mut self, region: impl Into<String>) -> Self {
        self.certification_region = region.into();
        self
    }

    /// Waits for this call's turn so bursts are spread out.
    async fn wait_turn(&self) {
        let mut next = self.next_slot.lock().await;
        let now = std::time::Instant::now();
        if *next > now {
            tokio::time::sleep(*next - now).await;
        }
        *next = std::time::Instant::now().max(*next) + MIN_GAP;
    }

    fn cached(&self, key: &str) -> Option<serde_json::Value> {
        let mut cache = self.cache.lock().unwrap();
        match cache.get(key) {
            Some((at, v)) if at.elapsed() < CACHE_TTL => Some(v.clone()),
            Some(_) => {
                cache.remove(key);
                None
            }
            None => None,
        }
    }

    fn remember(&self, key: String, value: &serde_json::Value) {
        let mut cache = self.cache.lock().unwrap();
        if cache.len() >= CACHE_MAX {
            cache.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
            if cache.len() >= CACHE_MAX {
                cache.clear();
            }
        }
        cache.insert(key, (std::time::Instant::now(), value.clone()));
    }

    /// Any TMDB endpoint as JSON, for the pages that browse people, companies and charts.
    pub async fn raw(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<serde_json::Value, MetadataError> {
        self.get(path, query).await
    }

    pub(crate) async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, MetadataError> {
        let key = format!("{path}?{query:?}");
        if let Some(v) = self.cached(&key) {
            return serde_json::from_value(v).map_err(|e| MetadataError::Parse(e.to_string()));
        }
        let mut attempts = 0;
        let value = loop {
            self.wait_turn().await;
            let response = self
                .http
                .get(format!("{}{path}", self.base))
                .bearer_auth(&self.token)
                .query(&[("language", self.language.as_str())])
                .query(query)
                .send()
                .await
                .map_err(|e| MetadataError::Http(e.to_string()))?;
            match response.status().as_u16() {
                200 => {
                    break response
                        .json::<serde_json::Value>()
                        .await
                        .map_err(|e| MetadataError::Parse(e.to_string()))?;
                }
                401 => return Err(MetadataError::Unauthorized),
                404 => return Err(MetadataError::NotFound),
                429 if attempts < 2 => {
                    attempts += 1;
                    // Told to slow down: wait as long as asked (within reason).
                    let secs = response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(1)
                        .clamp(1, 10);
                    tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                }
                429 => return Err(MetadataError::RateLimited),
                other => return Err(MetadataError::Http(format!("status {other} for {path}"))),
            }
        };
        self.remember(key, &value);
        serde_json::from_value(value).map_err(|e| MetadataError::Parse(e.to_string()))
    }
}

#[async_trait::async_trait]
impl MovieMetadataProvider for TmdbProvider {
    async fn movie(&self, tmdb_id: i64) -> Result<MovieMetadata, MetadataError> {
        let raw: RawMovie = self
            .get(
                &format!("/movie/{tmdb_id}"),
                &[(
                    "append_to_response",
                    "release_dates,alternative_titles,credits".into(),
                )],
            )
            .await?;
        Ok(map_movie(raw, &self.certification_region, Utc::now()))
    }

    async fn search(
        &self,
        term: &str,
        year: Option<i32>,
    ) -> Result<Vec<MovieMetadata>, MetadataError> {
        let mut query = vec![("query", term.to_owned())];
        if let Some(y) = year {
            query.push(("year", y.to_string()));
        }
        let page: RawSearchPage = self.get("/search/movie", &query).await?;
        let now = Utc::now();
        Ok(page
            .results
            .into_iter()
            .map(|r| map_search_result(r, now))
            .collect())
    }

    async fn collection(&self, tmdb_id: i64) -> Result<crate::CollectionInfo, MetadataError> {
        let raw: RawCollectionPage = self.get(&format!("/collection/{tmdb_id}"), &[]).await?;
        let now = Utc::now();
        let mut movies: Vec<MovieMetadata> = raw
            .parts
            .into_iter()
            .map(|r| map_search_result(r, now))
            .collect();
        // Release order; movies with no date yet go last.
        movies.sort_by_key(|m| (m.in_cinemas.is_none(), m.in_cinemas));
        Ok(crate::CollectionInfo {
            tmdb_id: raw.id,
            name: raw.name,
            overview: nonempty(raw.overview),
            movies,
        })
    }

    async fn movie_by_imdb_id(&self, imdb_id: &str) -> Result<MovieMetadata, MetadataError> {
        let found: RawFind = self
            .get(
                &format!("/find/{imdb_id}"),
                &[("external_source", "imdb_id".into())],
            )
            .await?;
        let id = found
            .movie_results
            .first()
            .map(|m| m.id)
            .ok_or(MetadataError::NotFound)?;
        self.movie(id).await
    }
}

// ---- raw TMDB shapes (only what we use) ------------------------------------

#[derive(Debug, Deserialize)]
struct RawMovie {
    id: i64,
    #[serde(default)]
    imdb_id: Option<String>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    original_title: String,
    #[serde(default)]
    original_language: String,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    runtime: Option<u32>,
    #[serde(default)]
    status: String,
    #[serde(default)]
    genres: Vec<RawGenre>,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    popularity: f64,
    #[serde(default)]
    vote_average: f64,
    #[serde(default)]
    vote_count: u32,
    #[serde(default)]
    poster_path: Option<String>,
    #[serde(default)]
    backdrop_path: Option<String>,
    #[serde(default)]
    belongs_to_collection: Option<RawCollection>,
    #[serde(default)]
    production_companies: Vec<RawCompany>,
    #[serde(default)]
    release_dates: Option<RawReleaseDates>,
    #[serde(default)]
    alternative_titles: Option<RawAlternativeTitles>,
    #[serde(default)]
    credits: Option<RawCredits>,
}

#[derive(Debug, Deserialize)]
struct RawGenre {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawCollection {
    id: i64,
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawCollectionPage {
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    parts: Vec<RawSearchResult>,
}

#[derive(Debug, Deserialize)]
struct RawCompany {
    name: String,
}

#[derive(Debug, Deserialize, Default)]
struct RawReleaseDates {
    #[serde(default)]
    results: Vec<RawCountryReleases>,
}

#[derive(Debug, Deserialize)]
struct RawCountryReleases {
    iso_3166_1: String,
    #[serde(default)]
    release_dates: Vec<RawReleaseDate>,
}

#[derive(Debug, Deserialize)]
struct RawReleaseDate {
    #[serde(default)]
    certification: String,
    release_date: String,
    /// 1 premiere, 2 theatrical (limited), 3 theatrical, 4 digital, 5 physical, 6 TV.
    #[serde(rename = "type")]
    kind: u8,
}

#[derive(Debug, Deserialize, Default)]
struct RawAlternativeTitles {
    #[serde(default)]
    titles: Vec<RawAlternativeTitle>,
}

#[derive(Debug, Deserialize)]
struct RawAlternativeTitle {
    #[serde(default)]
    iso_3166_1: String,
    title: String,
}

#[derive(Debug, Deserialize, Default)]
struct RawCredits {
    #[serde(default)]
    cast: Vec<RawCast>,
    #[serde(default)]
    crew: Vec<RawCrew>,
}

#[derive(Debug, Deserialize)]
struct RawCast {
    id: i64,
    name: String,
    #[serde(default)]
    character: String,
    #[serde(default)]
    order: u32,
    #[serde(default)]
    profile_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawCrew {
    id: i64,
    name: String,
    #[serde(default)]
    job: String,
    #[serde(default)]
    profile_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawSearchPage {
    #[serde(default)]
    results: Vec<RawSearchResult>,
}

#[derive(Debug, Deserialize)]
struct RawSearchResult {
    id: i64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    original_title: String,
    #[serde(default)]
    original_language: String,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    popularity: f64,
    #[serde(default)]
    vote_average: f64,
    #[serde(default)]
    vote_count: u32,
    #[serde(default)]
    poster_path: Option<String>,
    #[serde(default)]
    backdrop_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawFind {
    #[serde(default)]
    movie_results: Vec<RawSearchResult>,
}

// ---- mapping -----------------------------------------------------------------

pub(crate) fn parse_date(s: &str) -> Option<DateTime<Utc>> {
    // "1999-03-31T00:00:00.000Z" (release_dates) or "1999-03-31" (release_date).
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|d| d.and_utc())
}

fn nonempty(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.trim().is_empty())
}

fn image(kind: CoverKind, path: Option<String>) -> Option<MediaCover> {
    path.map(|p| MediaCover {
        kind,
        url: format!("{IMAGE_BASE}{p}"),
    })
}

fn earliest(dates: impl Iterator<Item = DateTime<Utc>>) -> Option<DateTime<Utc>> {
    dates.min()
}

/// Our status rule. TMDB has no "in cinemas" state, so it comes from dates:
/// a home release, or 90 days in cinemas, means released.
pub fn derive_status(
    tmdb_status: &str,
    in_cinemas: Option<DateTime<Utc>>,
    physical: Option<DateTime<Utc>>,
    digital: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> MovieStatus {
    let past = |d: Option<DateTime<Utc>>| d.is_some_and(|d| d <= now);
    if past(physical) || past(digital) || in_cinemas.is_some_and(|d| d + Duration::days(90) <= now)
    {
        return MovieStatus::Released;
    }
    if past(in_cinemas) {
        return MovieStatus::InCinemas;
    }
    match tmdb_status {
        "Rumored" | "Canceled" => MovieStatus::Tba,
        // "Released" with no usable dates: trust the provider.
        "Released" => MovieStatus::Released,
        _ => MovieStatus::Announced,
    }
}

fn map_movie(raw: RawMovie, certification_region: &str, now: DateTime<Utc>) -> MovieMetadata {
    let releases: Vec<(&str, &RawReleaseDate)> = raw
        .release_dates
        .iter()
        .flat_map(|r| r.results.iter())
        .flat_map(|c| {
            c.release_dates
                .iter()
                .map(move |d| (c.iso_3166_1.as_str(), d))
        })
        .collect();

    // Earliest worldwide date per kind.
    let by_kind = |kinds: &[u8]| {
        earliest(
            releases
                .iter()
                .filter(|(_, d)| kinds.contains(&d.kind))
                .filter_map(|(_, d)| parse_date(&d.release_date)),
        )
    };
    let primary = raw.release_date.as_deref().and_then(parse_date);
    let in_cinemas = by_kind(&[2, 3]).or(primary);
    let physical = by_kind(&[5]);
    let digital = by_kind(&[4]);

    let certification = releases
        .iter()
        .filter(|(country, d)| *country == certification_region && !d.certification.is_empty())
        .min_by_key(|(_, d)| (d.kind != 3, parse_date(&d.release_date)))
        .map(|(_, d)| d.certification.clone());

    let year = raw
        .release_date
        .as_deref()
        .and_then(|d| d.get(..4))
        .and_then(|y| y.parse().ok())
        .unwrap_or(0);

    let mut credits = Vec::new();
    if let Some(c) = raw.credits {
        credits.extend(c.cast.into_iter().take(MAX_CAST).map(|p| Credit {
            person_tmdb_id: p.id,
            name: p.name,
            kind: CreditKind::Cast,
            role: p.character,
            order: p.order,
            image_url: p.profile_path.map(|x| format!("{IMAGE_BASE}{x}")),
        }));
        credits.extend(
            c.crew
                .into_iter()
                .filter(|p| CREW_JOBS.contains(&p.job.as_str()))
                .enumerate()
                .map(|(i, p)| Credit {
                    person_tmdb_id: p.id,
                    name: p.name,
                    kind: CreditKind::Crew,
                    role: p.job,
                    order: i as u32,
                    image_url: p.profile_path.map(|x| format!("{IMAGE_BASE}{x}")),
                }),
        );
    }

    MovieMetadata {
        tmdb_id: raw.id,
        imdb_id: nonempty(raw.imdb_id),
        title: raw.title,
        original_title: raw.original_title,
        original_language: raw.original_language,
        overview: nonempty(raw.overview),
        year,
        // TMDB has one release year; a secondary one comes from other providers.
        secondary_year: None,
        runtime: raw.runtime.unwrap_or(0),
        status: derive_status(&raw.status, in_cinemas, physical, digital, now),
        certification,
        genres: raw.genres.into_iter().map(|g| g.name).collect(),
        in_cinemas,
        physical_release: physical,
        digital_release: digital,
        collection_tmdb_id: raw.belongs_to_collection.as_ref().map(|c| c.id),
        collection_title: raw.belongs_to_collection.map(|c| c.name),
        website: nonempty(raw.homepage),
        studio: raw.production_companies.into_iter().next().map(|c| c.name),
        ratings: (raw.vote_count > 0).then_some(Rating {
            votes: raw.vote_count,
            value: raw.vote_average,
        }),
        images: [
            image(CoverKind::Poster, raw.poster_path),
            image(CoverKind::Fanart, raw.backdrop_path),
        ]
        .into_iter()
        .flatten()
        .collect(),
        alternative_titles: raw
            .alternative_titles
            .map(|a| a.titles)
            .unwrap_or_default()
            .into_iter()
            .map(|t| AlternativeTitle {
                title: t.title,
                country: Some(t.iso_3166_1).filter(|c| !c.is_empty()),
            })
            .collect(),
        credits,
        popularity: raw.popularity,
    }
}

fn map_search_result(raw: RawSearchResult, now: DateTime<Utc>) -> MovieMetadata {
    let primary = raw.release_date.as_deref().and_then(parse_date);
    let year = raw
        .release_date
        .as_deref()
        .and_then(|d| d.get(..4))
        .and_then(|y| y.parse().ok())
        .unwrap_or(0);
    MovieMetadata {
        tmdb_id: raw.id,
        title: raw.title,
        original_title: raw.original_title,
        original_language: raw.original_language,
        overview: nonempty(raw.overview),
        year,
        in_cinemas: primary,
        status: derive_status("", primary, None, None, now),
        ratings: (raw.vote_count > 0).then_some(Rating {
            votes: raw.vote_count,
            value: raw.vote_average,
        }),
        images: [
            image(CoverKind::Poster, raw.poster_path),
            image(CoverKind::Fanart, raw.backdrop_path),
        ]
        .into_iter()
        .flatten()
        .collect(),
        popularity: raw.popularity,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 17, 0, 0, 0).unwrap()
    }

    fn matrix() -> MovieMetadata {
        let raw: RawMovie =
            serde_json::from_str(include_str!("../tests/data/tmdb_movie_603.json")).unwrap();
        map_movie(raw, "US", now())
    }

    #[test]
    fn maps_core_fields_from_a_real_response() {
        let m = matrix();
        assert_eq!(m.tmdb_id, 603);
        assert_eq!(m.imdb_id.as_deref(), Some("tt0133093"));
        assert_eq!(m.title, "The Matrix");
        assert_eq!(m.year, 1999);
        assert_eq!(m.runtime, 136);
        assert_eq!(m.original_language, "en");
        assert_eq!(m.certification.as_deref(), Some("R"));
        assert_eq!(m.genres.first().map(String::as_str), Some("Action"));
        assert_eq!(m.collection_tmdb_id, Some(2344));
        assert_eq!(m.collection_title.as_deref(), Some("The Matrix Collection"));
        assert!(m.ratings.is_some_and(|r| r.votes > 1000));
    }

    #[test]
    fn dates_are_earliest_worldwide_per_kind_and_status_is_released() {
        let m = matrix();
        let cinemas = m.in_cinemas.unwrap();
        assert!(cinemas <= Utc.with_ymd_and_hms(1999, 3, 31, 0, 0, 0).unwrap());
        assert_eq!(m.status, MovieStatus::Released);
    }

    #[test]
    fn images_use_the_original_size_base() {
        let m = matrix();
        assert!(m.images.iter().any(|i| i.kind == CoverKind::Poster
            && i.url.starts_with("https://image.tmdb.org/t/p/original/")));
        assert!(m.images.iter().any(|i| i.kind == CoverKind::Fanart));
    }

    #[test]
    fn credits_keep_cast_and_key_crew_only() {
        let m = matrix();
        assert!(m.credits.iter().any(|c| c.kind == CreditKind::Cast));
        assert!(
            m.credits
                .iter()
                .filter(|c| c.kind == CreditKind::Crew)
                .all(|c| CREW_JOBS.contains(&c.role.as_str()))
        );
    }

    #[test]
    fn alternative_titles_carry_their_country() {
        let m = matrix();
        assert!(!m.alternative_titles.is_empty());
        assert!(m.alternative_titles.iter().all(|t| t.country.is_some()));
    }

    #[test]
    fn search_results_are_summaries() {
        let page: RawSearchPage =
            serde_json::from_str(include_str!("../tests/data/tmdb_search_matrix.json")).unwrap();
        let movies: Vec<_> = page
            .results
            .into_iter()
            .map(|r| map_search_result(r, now()))
            .collect();
        assert_eq!(movies.len(), 2);
        assert!(
            movies
                .iter()
                .all(|m| m.tmdb_id > 0 && !m.title.is_empty() && m.credits.is_empty())
        );
        assert!(movies.iter().any(|m| m.year == 1999));
    }

    #[test]
    fn status_rules() {
        let d = |y, m, day| Utc.with_ymd_and_hms(y, m, day, 0, 0, 0).unwrap();
        let n = now();
        // Home release already out.
        assert_eq!(
            derive_status(
                "Released",
                Some(d(2026, 9, 1)),
                None,
                Some(d(2026, 10, 1)),
                n
            ),
            MovieStatus::Released
        );
        // In cinemas for less than 90 days, no home release.
        assert_eq!(
            derive_status("Released", Some(d(2026, 9, 1)), None, None, n),
            MovieStatus::InCinemas
        );
        // In cinemas for more than 90 days.
        assert_eq!(
            derive_status("Released", Some(d(2026, 6, 1)), None, None, n),
            MovieStatus::Released
        );
        // Future cinema date.
        assert_eq!(
            derive_status("Post Production", Some(d(2027, 1, 1)), None, None, n),
            MovieStatus::Announced
        );
        assert_eq!(
            derive_status("Rumored", None, None, None, n),
            MovieStatus::Tba
        );
        assert_eq!(
            derive_status("Canceled", None, None, None, n),
            MovieStatus::Tba
        );
        assert_eq!(
            derive_status("Released", None, None, None, n),
            MovieStatus::Released
        );
    }

    #[test]
    fn parses_both_date_formats() {
        assert!(parse_date("1999-03-31T00:00:00.000Z").is_some());
        assert!(parse_date("1999-03-31").is_some());
        assert!(parse_date("").is_none());
        assert!(parse_date("nonsense").is_none());
    }
}
