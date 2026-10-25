use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use omp_metadata::{MovieMetadataProvider, tmdb::TmdbProvider};

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

const MOVIE: &str = r#"{"id":603,"title":"The Matrix","original_title":"The Matrix","release_date":"1999-03-30","runtime":136,"status":"Released","genres":[],"poster_path":null,"backdrop_path":null}"#;

#[tokio::test]
async fn repeated_lookups_hit_the_cache_and_a_429_is_retried() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/movie/603",
            get(|State(n): State<Arc<AtomicUsize>>| async move {
                // The first answer tells the client to slow down.
                if n.fetch_add(1, Ordering::SeqCst) == 0 {
                    return (
                        StatusCode::TOO_MANY_REQUESTS,
                        [("retry-after", "1")],
                        String::new(),
                    )
                        .into_response();
                }
                ([("content-type", "application/json")], MOVIE).into_response()
            }),
        )
        .with_state(calls.clone());
    let base = serve(app).await;
    let tmdb = TmdbProvider::new("token").with_base_url(base);

    let first = tmdb.movie(603).await.unwrap();
    assert_eq!(first.title, "The Matrix");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "one refusal, one retry");

    let again = tmdb.movie(603).await.unwrap();
    assert_eq!(again.title, "The Matrix");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "the second lookup came from the cache"
    );
}

#[tokio::test]
async fn calls_are_spaced_out() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/movie/{id}",
            get(|State(n): State<Arc<AtomicUsize>>| async move {
                n.fetch_add(1, Ordering::SeqCst);
                ([("content-type", "application/json")], MOVIE)
            }),
        )
        .with_state(calls.clone());
    let tmdb = Arc::new(TmdbProvider::new("token").with_base_url(serve(app).await));
    let started = std::time::Instant::now();
    let mut tasks = Vec::new();
    for id in 1..=10 {
        let t = tmdb.clone();
        tasks.push(tokio::spawn(async move { t.movie(id).await.unwrap() }));
    }
    for t in tasks {
        t.await.unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 10);
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(9 * 40),
        "ten calls took only {:?}",
        started.elapsed()
    );
}
