//! Talks to the real TMDB API. Run with:
//!   cargo test -p omp-metadata --test live -- --ignored
//! Needs TMDB_READ_TOKEN in the environment or in `.env` at the repo root.

use omp_metadata::{MetadataError, MovieMetadataProvider, tmdb::TmdbProvider};

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
