//! TV series from TMDB: details, seasons and episodes.

use std::collections::HashMap;

use chrono::{NaiveDate, TimeZone, Utc};
use omp_core::{
    movie::{AlternativeTitle, CoverKind, MediaCover, Rating},
    tv::{EpisodeInfo, SeasonInfo, SeriesInfo, SeriesStatus, SeriesType},
};
use serde::Deserialize;

use crate::{MetadataError, SeriesMetadataProvider, tmdb::TmdbProvider};

const IMAGE_BASE: &str = "https://image.tmdb.org/t/p/original";
/// TMDB allows 20 appended sub-requests per call.
const SEASONS_PER_CALL: usize = 20;

#[async_trait::async_trait]
impl SeriesMetadataProvider for TmdbProvider {
    async fn series(&self, tmdb_id: i64) -> Result<SeriesInfo, MetadataError> {
        let raw: RawSeries = self
            .get(
                &format!("/tv/{tmdb_id}"),
                &[(
                    "append_to_response",
                    "external_ids,alternative_titles,content_ratings".into(),
                )],
            )
            .await?;
        let numbers: Vec<i32> = raw.seasons.iter().map(|s| s.season_number).collect();
        let mut seasons: Vec<RawSeason> = Vec::new();
        for chunk in numbers.chunks(SEASONS_PER_CALL) {
            let append = chunk
                .iter()
                .map(|n| format!("season/{n}"))
                .collect::<Vec<_>>()
                .join(",");
            let batch: RawSeasons = self
                .get(&format!("/tv/{tmdb_id}"), &[("append_to_response", append)])
                .await?;
            for n in chunk {
                if let Some(value) = batch.rest.get(&format!("season/{n}")) {
                    let season: RawSeason = serde_json::from_value(value.clone())
                        .map_err(|e| MetadataError::Parse(e.to_string()))?;
                    seasons.push(season);
                }
            }
        }
        Ok(map_series(raw, seasons, &self.certification_region()))
    }

    async fn search_series(
        &self,
        term: &str,
        year: Option<i32>,
    ) -> Result<Vec<SeriesInfo>, MetadataError> {
        let mut query = vec![("query", term.to_owned())];
        if let Some(y) = year {
            query.push(("first_air_date_year", y.to_string()));
        }
        let page: RawSearchPage = self.get("/search/tv", &query).await?;
        Ok(page.results.into_iter().map(map_search_result).collect())
    }

    async fn series_by_imdb_id(&self, imdb_id: &str) -> Result<SeriesInfo, MetadataError> {
        self.find(imdb_id, "imdb_id").await
    }

    async fn series_by_tvdb_id(&self, tvdb_id: i64) -> Result<SeriesInfo, MetadataError> {
        self.find(&tvdb_id.to_string(), "tvdb_id").await
    }
}

impl TmdbProvider {
    async fn find(&self, id: &str, source: &str) -> Result<SeriesInfo, MetadataError> {
        let found: RawFind = self
            .get(
                &format!("/find/{id}"),
                &[("external_source", source.to_owned())],
            )
            .await?;
        let tmdb_id = found
            .tv_results
            .first()
            .map(|r| r.id)
            .ok_or(MetadataError::NotFound)?;
        SeriesMetadataProvider::series(self, tmdb_id).await
    }
}

// ---- raw TMDB shapes --------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawSeries {
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    original_name: String,
    #[serde(default)]
    original_language: String,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    first_air_date: Option<String>,
    #[serde(default)]
    last_air_date: Option<String>,
    #[serde(default)]
    status: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    networks: Vec<RawNamed>,
    #[serde(default)]
    episode_run_time: Vec<u32>,
    #[serde(default)]
    genres: Vec<RawNamed>,
    #[serde(default)]
    seasons: Vec<RawSeasonSummary>,
    #[serde(default)]
    vote_average: f64,
    #[serde(default)]
    vote_count: u32,
    #[serde(default)]
    poster_path: Option<String>,
    #[serde(default)]
    backdrop_path: Option<String>,
    #[serde(default)]
    external_ids: Option<RawExternalIds>,
    #[serde(default)]
    alternative_titles: Option<RawAlternativeTitles>,
    #[serde(default)]
    content_ratings: Option<RawContentRatings>,
}

#[derive(Debug, Deserialize)]
struct RawNamed {
    name: String,
}

#[derive(Debug, Deserialize, Default)]
struct RawExternalIds {
    #[serde(default)]
    imdb_id: Option<String>,
    #[serde(default)]
    tvdb_id: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
struct RawAlternativeTitles {
    #[serde(default)]
    results: Vec<RawAlternativeTitle>,
}

#[derive(Debug, Deserialize)]
struct RawAlternativeTitle {
    #[serde(default)]
    iso_3166_1: String,
    title: String,
}

#[derive(Debug, Deserialize, Default)]
struct RawContentRatings {
    #[serde(default)]
    results: Vec<RawContentRating>,
}

#[derive(Debug, Deserialize)]
struct RawContentRating {
    iso_3166_1: String,
    #[serde(default)]
    rating: String,
}

#[derive(Debug, Deserialize)]
struct RawSeasonSummary {
    season_number: i32,
}

/// The answer to `append_to_response=season/1,season/2,...`.
#[derive(Debug, Deserialize)]
struct RawSeasons {
    // Alongside the series' own fields, so values are decoded one by one.
    #[serde(flatten)]
    rest: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct RawSeason {
    #[serde(default)]
    season_number: i32,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    poster_path: Option<String>,
    #[serde(default)]
    episodes: Vec<RawEpisode>,
}

#[derive(Debug, Deserialize, Clone)]
struct RawEpisode {
    id: i64,
    #[serde(default)]
    season_number: i32,
    episode_number: i32,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    air_date: Option<String>,
    #[serde(default)]
    runtime: Option<u32>,
    #[serde(default)]
    episode_type: Option<String>,
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
    name: String,
    #[serde(default)]
    original_name: String,
    #[serde(default)]
    original_language: String,
    #[serde(default)]
    overview: Option<String>,
    #[serde(default)]
    first_air_date: Option<String>,
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
    tv_results: Vec<RawSearchResult>,
}

// ---- mapping ---------------------------------------------------------------

fn nonempty(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.trim().is_empty())
}

fn date(s: Option<&str>) -> Option<NaiveDate> {
    s.and_then(|s| NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok())
}

fn image(kind: CoverKind, path: Option<String>) -> Option<MediaCover> {
    path.map(|p| MediaCover {
        kind,
        url: format!("{IMAGE_BASE}{p}"),
    })
}

fn status(raw: &str) -> SeriesStatus {
    match raw {
        "Ended" | "Canceled" => SeriesStatus::Ended,
        "In Production" | "Planned" | "Pilot" => SeriesStatus::Upcoming,
        _ => SeriesStatus::Continuing,
    }
}

/// TMDB's own TV "type" tells talk shows from scripted series; Japanese
/// animation is almost always followed by absolute number.
fn suggest_type(kind: &str, genres: &[String], language: &str) -> SeriesType {
    if matches!(kind, "Talk Show" | "News") {
        SeriesType::Daily
    } else if language == "ja" && genres.iter().any(|g| g == "Animation") {
        SeriesType::Anime
    } else {
        SeriesType::Standard
    }
}

fn map_episode(e: RawEpisode, season: i32) -> EpisodeInfo {
    let air_date = date(e.air_date.as_deref());
    EpisodeInfo {
        tmdb_id: e.id,
        season_number: if e.season_number == 0 && season != 0 {
            season
        } else {
            e.season_number
        },
        episode_number: e.episode_number,
        title: nonempty(e.name).unwrap_or_else(|| "TBA".into()),
        overview: nonempty(e.overview),
        air_date,
        air_date_utc: air_date
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .map(|d| Utc.from_utc_datetime(&d)),
        runtime: e.runtime.unwrap_or(0),
        finale_type: match e.episode_type.as_deref() {
            Some("finale") => Some("season".to_owned()),
            Some("mid_season") => Some("midseason".to_owned()),
            _ => None,
        },
    }
}

fn map_series(raw: RawSeries, seasons: Vec<RawSeason>, region: &str) -> SeriesInfo {
    let first_aired = date(raw.first_air_date.as_deref());
    let genres: Vec<String> = raw.genres.into_iter().map(|g| g.name).collect();
    let ids = raw.external_ids.unwrap_or_default();
    let suggested_type = suggest_type(&raw.kind, &genres, &raw.original_language);

    let mut season_infos = Vec::new();
    let mut episodes = Vec::new();
    for s in seasons {
        season_infos.push(SeasonInfo {
            season_number: s.season_number,
            title: nonempty(s.name),
            episode_count: s.episodes.len() as u32,
            images: image(CoverKind::Poster, s.poster_path)
                .into_iter()
                .collect(),
        });
        let number = s.season_number;
        episodes.extend(s.episodes.into_iter().map(|e| map_episode(e, number)));
    }
    episodes.sort_by_key(|e| (e.season_number, e.episode_number));

    SeriesInfo {
        tmdb_id: raw.id,
        tvdb_id: ids.tvdb_id.filter(|id| *id > 0),
        imdb_id: nonempty(ids.imdb_id),
        title: raw.name,
        original_title: raw.original_name,
        original_language: raw.original_language,
        overview: nonempty(raw.overview),
        year: first_aired.map_or(0, |d| chrono::Datelike::year(&d)),
        status: status(&raw.status),
        network: raw.networks.into_iter().next().map(|n| n.name),
        runtime: raw.episode_run_time.first().copied().unwrap_or(0),
        genres,
        certification: raw.content_ratings.and_then(|c| {
            c.results
                .into_iter()
                .find(|r| r.iso_3166_1 == region && !r.rating.is_empty())
                .map(|r| r.rating)
        }),
        first_aired,
        last_aired: date(raw.last_air_date.as_deref()),
        images: [
            image(CoverKind::Poster, raw.poster_path),
            image(CoverKind::Fanart, raw.backdrop_path),
        ]
        .into_iter()
        .flatten()
        .collect(),
        alternative_titles: raw
            .alternative_titles
            .unwrap_or_default()
            .results
            .into_iter()
            .map(|t| AlternativeTitle {
                title: t.title,
                country: nonempty(Some(t.iso_3166_1)),
            })
            .collect(),
        ratings: (raw.vote_count > 0).then_some(Rating {
            votes: raw.vote_count,
            value: raw.vote_average,
        }),
        suggested_type,
        seasons: season_infos,
        episodes,
    }
}

fn map_search_result(r: RawSearchResult) -> SeriesInfo {
    let first_aired = date(r.first_air_date.as_deref());
    SeriesInfo {
        tmdb_id: r.id,
        title: r.name,
        original_title: r.original_name,
        original_language: r.original_language,
        overview: nonempty(r.overview),
        year: first_aired.map_or(0, |d| chrono::Datelike::year(&d)),
        first_aired,
        ratings: (r.vote_count > 0).then_some(Rating {
            votes: r.vote_count,
            value: r.vote_average,
        }),
        images: [
            image(CoverKind::Poster, r.poster_path),
            image(CoverKind::Fanart, r.backdrop_path),
        ]
        .into_iter()
        .flatten()
        .collect(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breaking_bad() -> SeriesInfo {
        let raw: RawSeries =
            serde_json::from_str(include_str!("../tests/data/tmdb_tv_1396.json")).unwrap();
        let season = |json: &str| serde_json::from_str::<RawSeason>(json).unwrap();
        map_series(
            raw,
            vec![
                season(include_str!("../tests/data/tmdb_tv_1396_season_0.json")),
                season(include_str!("../tests/data/tmdb_tv_1396_season_1.json")),
            ],
            "US",
        )
    }

    #[test]
    fn maps_the_series() {
        let s = breaking_bad();
        assert_eq!(
            (s.tmdb_id, s.title.as_str(), s.year),
            (1396, "Breaking Bad", 2008)
        );
        assert_eq!(s.imdb_id.as_deref(), Some("tt0903747"));
        assert_eq!(s.tvdb_id, Some(81189));
        assert_eq!(s.status, SeriesStatus::Ended);
        assert_eq!(s.network.as_deref(), Some("AMC"));
        assert_eq!(s.suggested_type, SeriesType::Standard);
        assert!(s.genres.iter().any(|g| g == "Drama"));
        assert!(s.images.iter().any(|i| i.kind == CoverKind::Poster));
        assert!(!s.alternative_titles.is_empty());
        assert!(s.ratings.is_some());
    }

    #[test]
    fn episodes_come_with_their_seasons_in_order() {
        let s = breaking_bad();
        assert_eq!(s.seasons.len(), 2);
        let first = s
            .episodes
            .iter()
            .find(|e| e.season_number == 1 && e.episode_number == 1)
            .unwrap();
        assert_eq!(first.title, "Pilot");
        assert_eq!(first.air_date, NaiveDate::from_ymd_opt(2008, 1, 20));
        assert_eq!(first.runtime, 59);
        let order: Vec<_> = s
            .episodes
            .iter()
            .map(|e| (e.season_number, e.episode_number))
            .collect();
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(order, sorted);
        // Specials are season 0.
        assert!(s.episodes.iter().any(|e| e.season_number == 0));
    }

    #[test]
    fn search_results_are_summaries() {
        let page: RawSearchPage = serde_json::from_str(include_str!(
            "../tests/data/tmdb_search_tv_breaking_bad.json"
        ))
        .unwrap();
        let found: Vec<_> = page.results.into_iter().map(map_search_result).collect();
        assert!(!found.is_empty());
        assert_eq!(found[0].tmdb_id, 1396);
        assert!(
            found
                .iter()
                .all(|s| s.episodes.is_empty() && !s.title.is_empty())
        );
    }

    #[test]
    fn find_results_name_the_series() {
        let found: RawFind =
            serde_json::from_str(include_str!("../tests/data/tmdb_find_tv.json")).unwrap();
        assert_eq!(found.tv_results[0].id, 1396);
    }

    #[test]
    fn status_and_type_rules() {
        assert_eq!(status("Returning Series"), SeriesStatus::Continuing);
        assert_eq!(status("Canceled"), SeriesStatus::Ended);
        assert_eq!(status("In Production"), SeriesStatus::Upcoming);
        let genres = |g: &[&str]| g.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(suggest_type("Talk Show", &[], "en"), SeriesType::Daily);
        assert_eq!(
            suggest_type("Scripted", &genres(&["Animation"]), "ja"),
            SeriesType::Anime
        );
        assert_eq!(
            suggest_type("Scripted", &genres(&["Animation"]), "en"),
            SeriesType::Standard
        );
    }
}
