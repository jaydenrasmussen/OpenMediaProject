use std::sync::Arc;

use async_trait::async_trait;
use axum::{Router, http::StatusCode, routing::get};
use omp_core::tv::{EpisodeInfo, SeasonInfo, SeriesInfo, SeriesStatus};
use omp_metadata::{MetadataError, SeriesMetadataProvider, skyhook::SkyhookSeriesProvider};

struct Tmdb;

fn tmdb_info() -> SeriesInfo {
    SeriesInfo {
        tmdb_id: 1396,
        tvdb_id: Some(81189),
        title: "Breaking Bad".into(),
        original_language: "en".into(),
        overview: Some("From TMDB".into()),
        status: SeriesStatus::Continuing,
        seasons: vec![SeasonInfo {
            season_number: 1,
            title: Some("S1".into()),
            episode_count: 1,
            images: vec![],
        }],
        episodes: vec![EpisodeInfo {
            tmdb_id: 5,
            season_number: 1,
            episode_number: 1,
            title: "TMDB pilot".into(),
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[async_trait]
impl SeriesMetadataProvider for Tmdb {
    async fn series(&self, tmdb_id: i64) -> Result<SeriesInfo, MetadataError> {
        if tmdb_id == 1396 {
            Ok(tmdb_info())
        } else {
            Err(MetadataError::NotFound)
        }
    }
    async fn search_series(
        &self,
        _: &str,
        _: Option<i32>,
    ) -> Result<Vec<SeriesInfo>, MetadataError> {
        Ok(vec![tmdb_info()])
    }
    async fn series_by_imdb_id(&self, _: &str) -> Result<SeriesInfo, MetadataError> {
        Ok(tmdb_info())
    }
    async fn series_by_tvdb_id(&self, _: i64) -> Result<SeriesInfo, MetadataError> {
        Ok(tmdb_info())
    }
}

const SHOW: &str = r#"{"tvdbId":81189,"tmdbId":1396,"status":"Ended","runtime":48,
 "alternativeTitles":[{"title":"BB"},{"title":"Breaking Bad"}],
 "seasons":[{"seasonNumber":1,"images":[{"coverType":"Poster","url":"https://img/s1.jpg"},{"coverType":"Banner","url":"https://img/b.jpg"}]}],
 "episodes":[
  {"tvdbId":11,"seasonNumber":0,"episodeNumber":1,"title":"Special","airDate":"2009-02-17","airDateUtc":"2009-02-18T02:00:00Z","runtime":3},
  {"tvdbId":12,"seasonNumber":1,"episodeNumber":1,"title":"Pilot","airDate":"2008-01-20","airDateUtc":"2008-01-21T02:00:00Z","runtime":58,"finaleType":"season"},
  {"tvdbId":13,"seasonNumber":1,"episodeNumber":2,"airDate":"bad"}]}"#;

async fn serve(status: StatusCode, body: &'static str) -> String {
    let app = Router::new().route("/shows/en/{id}", get(move || async move { (status, body) }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    base
}

#[tokio::test]
async fn skyhook_episodes_are_laid_over_the_wrapped_providers_series() {
    let base = serve(StatusCode::OK, SHOW).await;
    let p = SkyhookSeriesProvider::new(Arc::new(Tmdb)).with_base_url(base);
    let s = p.series(1396).await.unwrap();
    assert_eq!(s.overview.as_deref(), Some("From TMDB"), "the rest stays");
    assert_eq!((s.status, s.runtime), (SeriesStatus::Ended, 48));
    assert_eq!(s.episodes.len(), 3);
    assert_eq!(s.episodes[1].title, "Pilot");
    assert_eq!(s.episodes[1].finale_type.as_deref(), Some("season"));
    assert_eq!(s.episodes[2].title, "TBA");
    assert!(s.episodes[2].air_date.is_none());
    let counts: Vec<_> = s
        .seasons
        .iter()
        .map(|x| (x.season_number, x.episode_count))
        .collect();
    assert_eq!(counts, [(0, 1), (1, 2)]);
    assert_eq!(s.seasons[1].title.as_deref(), Some("S1"));
    assert_eq!(s.seasons[1].images.len(), 1, "banners are not used");
    assert_eq!(s.alternative_titles.len(), 1, "own title not repeated");
}

#[tokio::test]
async fn a_down_or_disagreeing_skyhook_falls_back_quietly() {
    for (status, body) in [
        (StatusCode::INTERNAL_SERVER_ERROR, ""),
        (StatusCode::NOT_FOUND, ""),
        (StatusCode::OK, "not json"),
    ] {
        let base = serve(status, body).await;
        let p = SkyhookSeriesProvider::new(Arc::new(Tmdb)).with_base_url(base);
        assert_eq!(
            p.series(1396).await.unwrap().episodes[0].title,
            "TMDB pilot"
        );
    }
    // It describes another TMDB series: not ours.
    let base = serve(
        StatusCode::OK,
        r#"{"tmdbId":999,"episodes":[{"seasonNumber":1,"episodeNumber":1}]}"#,
    )
    .await;
    let p = SkyhookSeriesProvider::new(Arc::new(Tmdb)).with_base_url(base);
    assert_eq!(
        p.series(1396).await.unwrap().episodes[0].title,
        "TMDB pilot"
    );
    assert!(
        matches!(p.series(5).await, Err(MetadataError::NotFound)),
        "the wrapped provider's errors pass through"
    );
}
