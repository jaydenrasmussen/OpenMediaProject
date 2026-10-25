//! Browsing TMDB through the API: the Discover page, paged search, a title's
//! cast and crew, a person's works and a company's works. TMDB is a small fake.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query},
    http::{Method, Request, StatusCode},
    routing::get,
};
use http_body_util::BodyExt;
use omp_app::{App, AppConfig, router};
use omp_core::movie::{MovieMetadata, MovieStatus};
use omp_db::{Db, repo::NewMovie};
use omp_services::{MediaProbe, NoDiskCheck, ProbeInfo};
use serde_json::{Value, json};
use tower::ServiceExt;

struct NoProbe;

#[async_trait]
impl MediaProbe for NoProbe {
    async fn probe(&self, _: &std::path::Path) -> Option<ProbeInfo> {
        None
    }
}

fn movie(id: i64, title: &str) -> Value {
    json!({
        "id": id, "title": title, "release_date": "1999-03-31", "overview": "A story.",
        "poster_path": format!("/p{id}.jpg"), "backdrop_path": format!("/b{id}.jpg"),
        "vote_average": 8.1
    })
}

async fn fake_tmdb() -> String {
    let app = Router::new()
        .route(
            "/trending/all/day",
            get(|| async {
                Json(json!({ "results": [
                    { "id": 9, "media_type": "person", "name": "Someone", "profile_path": "/x.jpg" },
                    { "id": 1, "media_type": "tv", "name": "No Backdrop" },
                    { "media_type": "movie", "id": 603, "title": "The Matrix", "overview": "Neo.",
                      "backdrop_path": "/m.jpg", "poster_path": "/mp.jpg", "release_date": "1999-03-31" },
                ]}))
            }),
        )
        .route(
            "/movie/popular",
            get(|| async { Json(json!({ "results": [movie(603, "The Matrix")] })) }),
        )
        .route(
            "/search/multi",
            get(|Query(q): Query<std::collections::HashMap<String, String>>| async move {
                let page: i64 = q["page"].parse().unwrap();
                Json(json!({
                    "page": page, "total_pages": 3, "total_results": 5,
                    "results": [ { "media_type": "movie", "id": 100 + page, "title": format!("Hit {page}") } ]
                }))
            }),
        )
        .route(
            "/movie/603",
            get(|| async {
                Json(json!({
                    "id": 603, "title": "The Matrix", "overview": "Neo.", "tagline": "Free your mind",
                    "release_date": "1999-03-31", "runtime": 136, "status": "Released",
                    "genres": [{ "name": "Action" }],
                    "production_companies": [{ "id": 7, "name": "Warner Bros.", "origin_country": "US", "logo_path": "/w.png" }],
                    "credits": {
                        "cast": [{ "id": 6384, "name": "Keanu Reeves", "character": "Neo", "profile_path": "/k.jpg" }],
                        "crew": [
                            { "id": 1, "name": "Lana Wachowski", "job": "Director" },
                            { "id": 1, "name": "Lana Wachowski", "job": "Writer" },
                            { "id": 2, "name": "Joel Silver", "job": "Producer" },
                            { "id": 3, "name": "Grip", "job": "Grip" }
                        ]
                    },
                    "recommendations": { "results": [movie(604, "The Matrix Reloaded")] },
                    "videos": { "results": [{ "site": "YouTube", "type": "Trailer", "key": "abc" }] }
                }))
            }),
        )
        .route(
            "/person/6384",
            get(|| async {
                Json(json!({
                    "id": 6384, "name": "Keanu Reeves", "biography": "Actor.", "birthday": "1964-09-02",
                    "known_for_department": "Acting", "profile_path": "/k.jpg",
                    "combined_credits": {
                        "cast": [
                            { "media_type": "movie", "id": 603, "title": "The Matrix", "release_date": "1999-03-31", "character": "Neo" },
                            { "media_type": "movie", "id": 624, "title": "Newer", "release_date": "2021-12-01", "character": "Neo" }
                        ],
                        "crew": [
                            { "media_type": "movie", "id": 700, "title": "Directed", "release_date": "2013-01-01", "job": "Director" },
                            { "media_type": "movie", "id": 701, "title": "Lit", "release_date": "2013-01-01", "job": "Gaffer" }
                        ]
                    }
                }))
            }),
        )
        .route(
            "/company/7",
            get(|| async { Json(json!({ "id": 7, "name": "Warner Bros.", "description": "A studio.", "logo_path": "/w.png", "origin_country": "US" })) }),
        )
        .route(
            "/discover/{kind}",
            get(|Path(kind): Path<String>, Query(q): Query<std::collections::HashMap<String, String>>| async move {
                assert_eq!(q["with_companies"], "7");
                Json(json!({
                    "page": q["page"].parse::<i64>().unwrap(), "total_pages": 2, "total_results": 40,
                    "results": [ if kind == "tv" { json!({ "id": 5, "name": "A Show", "first_air_date": "2010-01-01" }) } else { movie(603, "The Matrix") } ]
                }))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

struct Rig {
    app: Arc<App>,
    router: Router,
    key: String,
    _dir: tempfile::TempDir,
}

async fn rig(tmdb: Option<String>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut config = AppConfig::new(dir.path().join("data"));
    if let Some(base) = tmdb {
        config.tmdb_token = Some(format!("token-{base}"));
        config.tmdb_base_url = Some(base);
    }
    let app = App::new(
        config,
        Db::in_memory().await.unwrap(),
        None,
        Arc::new(NoProbe),
        Arc::new(NoDiskCheck),
    )
    .await
    .unwrap();
    let key = app.api_key().await.unwrap();
    Rig {
        router: router(app.clone()),
        app,
        key,
        _dir: dir,
    }
}

async fn get_json(r: &Rig, path: &str) -> (StatusCode, Value) {
    let res = r
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(path)
                .header("x-api-key", &r.key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn add_matrix(r: &Rig) -> i64 {
    let metadata_id = r
        .app
        .db
        .movies()
        .upsert_metadata(&MovieMetadata {
            tmdb_id: 603,
            title: "The Matrix".into(),
            original_title: "The Matrix".into(),
            original_language: "en".into(),
            year: 1999,
            status: MovieStatus::Released,
            ..Default::default()
        })
        .await
        .unwrap();
    let profile = r.app.db.quality_profiles().list().await.unwrap()[0].id;
    r.app
        .db
        .movies()
        .add(&NewMovie {
            metadata_id,
            monitored: true,
            minimum_availability: MovieStatus::Announced,
            quality_profile_id: profile,
            path: "/movies/The Matrix (1999)".into(),
            root_folder_path: None,
            tags: vec![],
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn discover_leads_with_a_title_that_has_a_backdrop_and_marks_what_is_owned() {
    let r = rig(Some(fake_tmdb().await)).await;
    let id = add_matrix(&r).await;
    let (s, home) = get_json(&r, "/api/v1/browse/home").await;
    assert_eq!(s, StatusCode::OK, "{home}");
    let hero = &home["hero"];
    assert_eq!(
        hero["title"], "The Matrix",
        "people and titles without a backdrop are passed over"
    );
    assert_eq!(hero["in_library"], id);
    assert_eq!(hero["images"][1]["kind"], "Fanart");
    assert_eq!(
        hero["images"][1]["url"],
        "https://image.tmdb.org/t/p/original/m.jpg"
    );
    // Only one chart is served by the fake; the others being down leaves it.
    let rows = home["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (&rows[0]["title"], &rows[0]["items"][0]["in_library"]),
        (&json!("Popular movies"), &json!(id))
    );
}

#[tokio::test]
async fn search_is_paged() {
    let r = rig(Some(fake_tmdb().await)).await;
    let (_, one) = get_json(&r, "/api/v1/browse/search?term=hit").await;
    let (_, two) = get_json(&r, "/api/v1/browse/search?term=hit&page=2").await;
    assert_eq!(
        (one["page"].as_i64(), one["total_pages"].as_i64()),
        (Some(1), Some(3))
    );
    assert_eq!(one["results"][0]["title"], "Hit 1");
    assert_eq!(two["results"][0]["title"], "Hit 2");
    assert_eq!(
        get_json(&r, "/api/v1/browse/search?term=").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get_json(&r, "/api/v1/browse/search?term=x&kind=nope")
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn a_title_carries_its_cast_crew_and_companies() {
    let r = rig(Some(fake_tmdb().await)).await;
    let (s, t) = get_json(&r, "/api/v1/browse/title/movie/603").await;
    assert_eq!(s, StatusCode::OK, "{t}");
    assert_eq!(t["tagline"], "Free your mind");
    assert_eq!(t["runtime"], 136);
    assert_eq!(
        (&t["cast"][0]["title"], &t["cast"][0]["role"]),
        (&json!("Keanu Reeves"), &json!("Neo"))
    );
    assert_eq!(t["cast"][0]["kind"], "person");
    // One person on two jobs is one entry; crew with other jobs are left out.
    assert_eq!(
        t["directors"],
        json!([{ "id": 1, "name": "Lana Wachowski", "job": "Director" }])
    );
    assert_eq!(t["writers"][0]["name"], "Lana Wachowski");
    assert_eq!(
        t["producers"],
        json!([{ "id": 2, "name": "Joel Silver", "job": "Producer" }])
    );
    assert_eq!(t["companies"][0]["id"], 7);
    assert_eq!(t["trailer"], "abc");
    assert_eq!(t["recommended"][0]["title"], "The Matrix Reloaded");
    assert_eq!(
        get_json(&r, "/api/v1/browse/title/book/1").await.0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn a_person_has_their_works_by_what_they_did_newest_first() {
    let r = rig(Some(fake_tmdb().await)).await;
    let id = add_matrix(&r).await;
    let (s, p) = get_json(&r, "/api/v1/browse/person/6384").await;
    assert_eq!(s, StatusCode::OK, "{p}");
    assert_eq!(p["biography"], "Actor.");
    let acting = p["credits"]["Acting"].as_array().unwrap();
    let titles: Vec<_> = acting
        .iter()
        .map(|w| w["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Newer", "The Matrix"]);
    assert_eq!(acting[1]["in_library"], id);
    assert_eq!(p["credits"]["Directing"][0]["title"], "Directed");
    assert!(
        p["credits"].get("Producing").is_none(),
        "empty sections are left out"
    );
    assert_eq!(
        p["credits"]["Directing"].as_array().unwrap().len(),
        1,
        "a gaffer credit is not directing"
    );
}

#[tokio::test]
async fn a_company_lists_its_movies_and_series_a_page_at_a_time() {
    let r = rig(Some(fake_tmdb().await)).await;
    let (s, c) = get_json(&r, "/api/v1/browse/company/7?page=2").await;
    assert_eq!(s, StatusCode::OK, "{c}");
    assert_eq!(c["company"]["name"], "Warner Bros.");
    assert_eq!(
        (c["page"].as_i64(), c["total_pages"].as_i64()),
        (Some(2), Some(2))
    );
    assert_eq!(c["results"][0]["kind"], "movie");
    let (_, tv) = get_json(&r, "/api/v1/browse/company/7?kind=tv").await;
    assert_eq!(tv["results"][0]["kind"], "tv");
}

#[tokio::test]
async fn browsing_says_what_it_needs_when_there_is_no_tmdb_key() {
    let r = rig(None).await;
    let (s, v) = get_json(&r, "/api/v1/browse/home").await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        v["error"].as_str().unwrap().contains("TMDB_READ_TOKEN"),
        "{v}"
    );
}
