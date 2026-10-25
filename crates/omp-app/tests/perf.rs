//! Large-library timings: 10,000 movies and 2,000 series of 40 episodes.
//! Slow to set up, so it only runs when asked:
//! `cargo test -p omp-app --release --test perf -- --ignored --nocapture`

use std::{path::Path, sync::Arc, time::Instant};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use omp_app::{App, AppConfig, router};
use omp_core::{
    movie::{MovieMetadata, MovieStatus},
    tv::{EpisodeInfo, SeriesInfo},
};
use omp_db::{
    Db,
    repo::{NewMovie, NewSeries},
};
use omp_services::{MediaProbe, NoDiskCheck, ProbeInfo};
use tower::ServiceExt;

struct NoProbe;

#[async_trait]
impl MediaProbe for NoProbe {
    async fn probe(&self, _: &Path) -> Option<ProbeInfo> {
        None
    }
}

fn rss_mb() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u64>()
        .unwrap_or(0)
        / 1024
}

#[tokio::test]
#[ignore = "builds a large library; run with --release"]
async fn a_large_library_lists_quickly_and_stays_small() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = AppConfig::new(dir.path().join("data"));
    config.ui_dir = dir.path().join("ui");
    let db = Db::open(&dir.path().join("data")).await.unwrap();
    // Creating the app seeds the default quality profiles.
    let app = App::new(
        config,
        db.clone(),
        None,
        Arc::new(NoProbe),
        Arc::new(NoDiskCheck),
    )
    .await
    .unwrap();
    let profile = db.quality_profiles().list().await.unwrap()[0].id;

    let started = Instant::now();
    for i in 0..10_000i64 {
        let meta = MovieMetadata {
            tmdb_id: 100_000 + i,
            title: format!("Movie Number {i}"),
            original_title: format!("Movie Number {i}"),
            year: 1950 + (i % 75) as i32,
            status: MovieStatus::Released,
            ..Default::default()
        };
        let metadata_id = db.movies().upsert_metadata(&meta).await.unwrap();
        db.movies()
            .add(&NewMovie {
                metadata_id,
                monitored: true,
                minimum_availability: MovieStatus::Announced,
                quality_profile_id: profile,
                path: format!("/media/Movie Number {i}"),
                root_folder_path: None,
                tags: vec![],
            })
            .await
            .unwrap();
    }
    for i in 0..2_000i64 {
        let info = SeriesInfo {
            tmdb_id: 500_000 + i,
            title: format!("Series Number {i}"),
            original_title: format!("Series Number {i}"),
            year: 1990 + (i % 35) as i32,
            ..Default::default()
        };
        let id = db
            .series()
            .add(&NewSeries {
                info,
                series_type: Default::default(),
                monitored: true,
                monitor_new_items: Default::default(),
                quality_profile_id: profile,
                season_folder: true,
                use_scene_numbering: false,
                path: format!("/tv/Series Number {i}"),
                root_folder_path: None,
                tags: vec![],
            })
            .await
            .unwrap();
        for n in 0..40 {
            db.episodes()
                .insert(
                    id,
                    &EpisodeInfo {
                        tmdb_id: i * 100 + n,
                        season_number: 1 + (n / 10) as i32,
                        episode_number: 1 + (n % 10) as i32,
                        title: format!("Episode {n}"),
                        ..Default::default()
                    },
                    None,
                    true,
                )
                .await
                .unwrap();
        }
    }
    println!("seeded in {:?}", started.elapsed());

    let key = app.api_key().await.unwrap();
    let r = router(app);
    for (path, limit_s) in [
        ("/api/v1/movie", 5.0),
        ("/api/v1/series", 10.0),
        ("/api/v1/wanted/missing?limit=50", 5.0),
    ] {
        let t = Instant::now();
        let res = r
            .clone()
            .oneshot(
                Request::get(path)
                    .header("x-api-key", &key)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{path}");
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let took = t.elapsed();
        println!("{path}: {:?}, {} KB", took, bytes.len() / 1024);
        assert!(took.as_secs_f64() < limit_s, "{path} took {took:?}");
    }
    let rss = rss_mb();
    println!("resident memory: {rss} MB");
    assert!(rss < 600, "resident memory {rss} MB");
}
