//! Talks to the real TMDB API. Run with:
//!   cargo test -p omp-metadata --test live -- --ignored
//! Needs TMDB_READ_TOKEN in the environment or in `.env` at the repo root.

use omp_metadata::{
    MetadataError, MovieMetadataProvider, SeriesMetadataProvider, tmdb::TmdbProvider,
};

fn provider() -> TmdbProvider {
    dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.env")).ok();
    TmdbProvider::from_env().expect("TMDB_READ_TOKEN is not set")
}

#[tokio::test]
#[ignore = "needs network and a TMDB token"]
async fn fetches_a_movie_by_tmdb_id() {
    let m = provider().movie(603).await.unwrap();
    assert_eq!(m.title, "The Matrix");
    assert_eq!(m.year, 1999);
    assert!(m.digital_release.is_some() || m.physical_release.is_some());
}

#[tokio::test]
#[ignore = "needs network and a TMDB token"]
async fn finds_by_imdb_id_and_searches() {
    let p = provider();
    assert_eq!(p.movie_by_imdb_id("tt0133093").await.unwrap().tmdb_id, 603);
    let hits = p.search("the matrix", Some(1999)).await.unwrap();
    assert!(hits.iter().any(|m| m.tmdb_id == 603));
}

#[tokio::test]
#[ignore = "needs network and a TMDB token"]
async fn unknown_id_is_not_found_and_bad_token_is_unauthorized() {
    assert!(matches!(
        provider().movie(999_999_999).await,
        Err(MetadataError::NotFound)
    ));
    let bad = TmdbProvider::new("not-a-real-token");
    assert!(matches!(
        bad.movie(603).await,
        Err(MetadataError::Unauthorized)
    ));
}

#[tokio::test]
#[ignore = "needs network and a TMDB token"]
async fn fetches_a_series_with_every_episode() {
    let p = provider();
    let s = p.series(1396).await.unwrap();
    assert_eq!(s.title, "Breaking Bad");
    assert_eq!(s.tvdb_id, Some(81189));
    // Five seasons and the specials, 62 aired episodes in all.
    assert_eq!(s.seasons.iter().filter(|x| x.season_number > 0).count(), 5);
    assert_eq!(
        s.episodes.iter().filter(|e| e.season_number > 0).count(),
        62
    );

    assert_eq!(
        p.series_by_imdb_id("tt0903747").await.unwrap().tmdb_id,
        1396
    );
    assert_eq!(p.series_by_tvdb_id(81189).await.unwrap().tmdb_id, 1396);
    let hits = p.search_series("breaking bad", None).await.unwrap();
    assert!(hits.iter().any(|x| x.tmdb_id == 1396));
}
