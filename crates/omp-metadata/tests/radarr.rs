use std::sync::Arc;

use async_trait::async_trait;
use axum::{Json, Router, http::StatusCode, routing::get};
use omp_core::movie::{CoverKind, CreditKind, MovieMetadata, MovieStatus};
use omp_metadata::{
    MetadataError, MovieMetadataProvider,
    radarr::{Fallback, RadarrMetaProvider},
};
use serde_json::{Value, json};

fn matrix() -> Value {
    json!({
        "TmdbId": 603, "ImdbId": "tt0133093", "Title": "The Matrix", "OriginalTitle": "The Matrix",
        "OriginalLanguage": "en", "Overview": "A hacker.", "Runtime": 136, "Popularity": 45.5, "Year": 1999,
        "InCinema": "1999-03-31T00:00:00Z", "PhysicalRelease": "1999-11-25T00:00:00Z",
        "DigitalRelease": "2001-03-19T00:00:00Z", "Status": null,
        "Images": [{"CoverType": "Poster", "Url": "https://img/p.jpg"}, {"CoverType": "Fanart", "Url": "https://img/f.jpg"}, {"CoverType": "Clearlogo", "Url": "https://img/l.png"}],
        "Ratings": [{"Count": 0, "Value": 7.0, "Origin": "Imdb"}, {"Count": 29031, "Value": 8.26, "Origin": "Tmdb"}],
        "Genres": ["Action", "Science Fiction"],
        "AlternativeTitles": [{"Title": "Matrix", "Type": "Tmdb", "Language": "de"}],
        "Credits": {
            "Cast": [{"Name": "Keanu Reeves", "Order": 0, "Character": "Neo", "TmdbId": 6384, "Images": [{"CoverType": "Headshot", "Url": "https://img/k.jpg"}]}],
            "Crew": [{"Name": "Lana Wachowski", "Order": 0, "Job": "Director", "TmdbId": 9340}, {"Name": "A Grip", "Order": 1, "Job": "Grip", "TmdbId": 1}]
        },
        "Certifications": [{"Country": "GB", "Certification": "15"}, {"Country": "US", "Certification": "R"}],
        "Studio": "Village Roadshow", "Homepage": "http://example",
        "Collection": {"TmdbId": 2344, "Name": "The Matrix Collection", "Parts": null}
    })
}

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn stub() -> RadarrMetaProvider {
    let app = Router::new()
        .route("/movie/603", get(|| async { Json(matrix()) }))
        .route("/movie/imdb/tt0133093", get(|| async { Json(json!([matrix()])) }))
        .route("/search", get(|| async { Json(json!([matrix(), {"Title": "bad", "TmdbId": 0}])) }))
        .route(
            "/movie/collection/2344",
            get(|| async {
                Json(json!({"TmdbId": 2344, "Name": "The Matrix Collection", "Overview": "Three films",
                    "Parts": [{"TmdbId": 605, "Title": "Revolutions", "Year": 2003, "InCinema": "2003-11-05T00:00:00Z"}, matrix()]}))
            }),
        )
        .route("/movie/404", get(|| async { StatusCode::NOT_FOUND }));
    RadarrMetaProvider::new().with_base_url(serve(app).await)
}

#[tokio::test]
async fn a_radarr_server_movie_maps_to_our_metadata() {
    let p = stub().await;
    let m = p.movie(603).await.unwrap();
    assert_eq!(
        (m.title.as_str(), m.year, m.runtime),
        ("The Matrix", 1999, 136)
    );
    assert_eq!(m.imdb_id.as_deref(), Some("tt0133093"));
    assert_eq!(m.certification.as_deref(), Some("R"), "the US one");
    assert_eq!(m.status, MovieStatus::Released);
    assert_eq!(
        m.ratings.as_ref().map(|r| r.votes),
        Some(29031),
        "TMDB's rating, not the empty one"
    );
    assert_eq!(
        m.images.iter().map(|i| i.kind).collect::<Vec<_>>(),
        [CoverKind::Poster, CoverKind::Fanart]
    );
    assert_eq!(
        (m.collection_tmdb_id, m.collection_title.as_deref()),
        (Some(2344), Some("The Matrix Collection"))
    );
    assert_eq!(m.credits.len(), 2, "the grip is not kept");
    assert_eq!(
        (m.credits[0].kind, m.credits[0].role.as_str()),
        (CreditKind::Cast, "Neo")
    );
    assert_eq!(m.credits[1].role, "Director");
    assert!(m.digital_release.is_some() && m.physical_release.is_some() && m.in_cinemas.is_some());

    assert!(matches!(p.movie(404).await, Err(MetadataError::NotFound)));
    assert_eq!(p.movie_by_imdb_id("tt0133093").await.unwrap().tmdb_id, 603);
    assert_eq!(
        p.search("matrix", Some(1999)).await.unwrap().len(),
        1,
        "an entry without an id is dropped"
    );
    let c = p.collection(2344).await.unwrap();
    assert_eq!(c.name, "The Matrix Collection");
    assert_eq!(
        c.movies.iter().map(|m| m.tmdb_id).collect::<Vec<_>>(),
        [603, 605],
        "release order"
    );
}

struct Down;

#[async_trait]
impl MovieMetadataProvider for Down {
    async fn movie(&self, _: i64) -> Result<MovieMetadata, MetadataError> {
        Err(MetadataError::Http("down".into()))
    }
    async fn search(&self, _: &str, _: Option<i32>) -> Result<Vec<MovieMetadata>, MetadataError> {
        Err(MetadataError::Http("down".into()))
    }
    async fn movie_by_imdb_id(&self, _: &str) -> Result<MovieMetadata, MetadataError> {
        Err(MetadataError::Http("down".into()))
    }
}

struct Backup;

#[async_trait]
impl MovieMetadataProvider for Backup {
    async fn movie(&self, id: i64) -> Result<MovieMetadata, MetadataError> {
        Ok(MovieMetadata {
            tmdb_id: id,
            title: "From TMDB".into(),
            ..Default::default()
        })
    }
    async fn search(&self, _: &str, _: Option<i32>) -> Result<Vec<MovieMetadata>, MetadataError> {
        Ok(vec![])
    }
    async fn movie_by_imdb_id(&self, _: &str) -> Result<MovieMetadata, MetadataError> {
        Err(MetadataError::NotFound)
    }
}

#[tokio::test]
async fn tmdb_answers_when_the_radarr_server_cannot() {
    // Dead server: connection refused.
    let dead = RadarrMetaProvider::new().with_base_url("http://127.0.0.1:1");
    let both = Fallback::new(Arc::new(dead), Arc::new(Backup));
    assert_eq!(both.movie(7).await.unwrap().title, "From TMDB");

    // A server that works wins.
    let both = Fallback::new(Arc::new(stub().await), Arc::new(Backup));
    assert_eq!(both.movie(603).await.unwrap().title, "The Matrix");
    // One it does not know is asked of TMDB.
    assert_eq!(both.movie(404).await.unwrap().title, "From TMDB");

    // Both down: the error shows.
    let none = Fallback::new(Arc::new(Down), Arc::new(Down));
    assert!(matches!(none.movie(1).await, Err(MetadataError::Http(_))));
    // Collections fall back to the default "not found" when nobody has them.
    assert!(matches!(
        Fallback::new(Arc::new(Down), Arc::new(Backup))
            .collection(1)
            .await,
        Err(MetadataError::NotFound)
    ));
}

/// Against the real server: `cargo test -p omp-metadata --test radarr -- --ignored`.
#[tokio::test]
#[ignore = "needs the internet"]
async fn the_real_server_knows_the_matrix() {
    let p = RadarrMetaProvider::new();
    let m = p.movie(603).await.unwrap();
    assert_eq!(m.title, "The Matrix");
    assert_eq!(m.collection_tmdb_id, Some(2344));
    assert!(!m.credits.is_empty() && m.certification.is_some());
    let c = p.collection(2344).await.unwrap();
    assert!(c.movies.len() >= 3, "{} parts", c.movies.len());
    assert!(!p.search("matrix", None).await.unwrap().is_empty());
}
