use std::sync::Arc;

use chrono::{DateTime, Duration, TimeZone, Utc};
use omp_core::{
    history::HistoryEvent,
    language::Language,
    movie::{MovieMetadata, MovieStatus},
    quality::Quality,
    release::DownloadProtocol,
};
use omp_db::{
    Db,
    repo::{NewMovie, Provider, ProviderKind},
};
use omp_downloads::{ClientError, ClientRegistry, FakeClient, RegisteredClient};
use omp_downloads::{ClientItem, DownloadItemStatus};
use omp_quality::profile::{ProfileItem, QualityProfile};
use omp_services::{DownloadMonitor, MonitorOptions, TrackedState};

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap()
}

async fn setup() -> Db {
    let db = Db::in_memory().await.unwrap();
    db.delay_profiles().seed_default().await.unwrap();
    let q = |quality, allowed| ProfileItem::Quality { quality, allowed };
    db.quality_profiles()
        .insert(&QualityProfile {
            id: 0,
            name: "HD".into(),
            cutoff: Quality::BLURAY_1080P.id,
            items: vec![
                q(Quality::DVD, true),
                q(Quality::WEBDL_720P, true),
                q(Quality::WEBDL_1080P, true),
                q(Quality::BLURAY_1080P, true),
            ],
            min_format_score: 0,
            cutoff_format_score: 0,
            min_upgrade_format_score: 1,
            format_items: vec![],
            language: Language::ANY,
            upgrade_allowed: true,
        })
        .await
        .unwrap();
    db
}

async fn add_movie(db: &Db, tmdb: i64, title: &str, year: i32) -> i64 {
    let profile = db.quality_profiles().list().await.unwrap()[0].id;
    let metadata_id = db
        .movies()
        .upsert_metadata(&MovieMetadata {
            tmdb_id: tmdb,
            title: title.into(),
            original_title: title.into(),
            original_language: "en".into(),
            year,
            runtime: 120,
            status: MovieStatus::Released,
            ..Default::default()
        })
        .await
        .unwrap();
    db.movies()
        .add(&NewMovie {
            metadata_id,
            monitored: true,
            minimum_availability: MovieStatus::Announced,
            quality_profile_id: profile,
            path: format!("/movies/{title}"),
            root_folder_path: None,
            tags: vec![],
        })
        .await
        .unwrap()
}

async fn client_row(db: &Db, name: &str, protocol: &str) -> i64 {
    db.providers()
        .insert(&Provider {
            id: 0,
            kind: ProviderKind::DownloadClient,
            name: name.into(),
            implementation: protocol.into(),
            plugin_id: None,
            settings: serde_json::json!({}),
            tags: vec![],
            enabled: true,
            enable_rss: false,
            enable_automatic_search: false,
            enable_interactive_search: false,
            priority: 1,
        })
        .await
        .unwrap()
}

fn item(id: &str, status: DownloadItemStatus) -> ClientItem {
    ClientItem {
        download_id: id.into(),
        category: Some("movies".into()),
        title: "The.Matrix.1999.1080p.BluRay.x264-GRP".into(),
        total_size: 100,
        remaining_size: 0,
        status,
        output_path: Some("/downloads/The.Matrix".into()),
        seed_ratio: None,
        can_move_files: true,
        can_be_removed: false,
        message: Some("par2 repair failed".into()),
        ..Default::default()
    }
}

fn opts() -> MonitorOptions {
    MonitorOptions {
        now: now(),
        category: Some("movies".into()),
        remove_failed: true,
    }
}

async fn grab_row(db: &Db, movie: i64, download_id: &str) {
    let mut data = std::collections::BTreeMap::new();
    data.insert("indexer".to_owned(), "Idx".to_owned());
    data.insert("size".to_owned(), "4000".to_owned());
    data.insert("published_date".to_owned(), now().to_rfc3339());
    db.history()
        .insert(&omp_db::repo::HistoryRecord {
            id: 0,
            movie_id: movie,
            event: HistoryEvent::Grabbed,
            date: now() - Duration::hours(1),
            source_title: "The.Matrix.1999.1080p.BluRay.x264-GRP".into(),
            quality: omp_core::quality::QualityModel {
                quality: Quality::BLURAY_1080P,
                ..Default::default()
            },
            languages: vec![],
            download_id: Some(download_id.into()),
            data,
        })
        .await
        .unwrap();
}

async fn rig() -> (Db, Arc<FakeClient>, ClientRegistry, i64) {
    let db = setup().await;
    let movie = add_movie(&db, 603, "The Matrix", 1999).await;
    let client = Arc::new(FakeClient::new(DownloadProtocol::Usenet));
    let id = client_row(&db, "SAB", "sabnzbd").await;
    let registry = ClientRegistry::new(vec![RegisteredClient {
        id,
        name: "SAB".into(),
        priority: 1,
        tags: vec![],
        client: client.clone(),
    }]);
    (db, client, registry, movie)
}

#[tokio::test]
async fn a_grabbed_download_is_tracked_and_becomes_import_pending_when_done() {
    let (db, client, registry, movie) = rig().await;
    grab_row(&db, movie, "d1").await;
    let monitor = DownloadMonitor::new();

    client.set_items(vec![item("d1", DownloadItemStatus::Downloading)]);
    let r = monitor.refresh(&db, &registry, &opts()).await.unwrap();
    let t = monitor.get("d1").unwrap();
    assert_eq!(
        (t.state, t.movie_id),
        (TrackedState::Downloading, Some(movie))
    );
    assert_eq!(t.indexer.as_deref(), Some("Idx"));
    assert!(r.newly_pending.is_empty());

    client.set_items(vec![item("d1", DownloadItemStatus::Completed)]);
    let r = monitor.refresh(&db, &registry, &opts()).await.unwrap();
    assert_eq!(
        monitor.get("d1").unwrap().state,
        TrackedState::ImportPending
    );
    assert_eq!(r.newly_pending, ["d1"]);

    // Reported again: still pending, not announced twice.
    let r = monitor.refresh(&db, &registry, &opts()).await.unwrap();
    assert!(r.newly_pending.is_empty());
}

#[tokio::test]
async fn a_failed_download_is_recorded_blocklisted_and_removed() {
    let (db, client, registry, movie) = rig().await;
    grab_row(&db, movie, "d1").await;
    let monitor = DownloadMonitor::new();
    client.set_items(vec![item("d1", DownloadItemStatus::Failed)]);

    let r = monitor.refresh(&db, &registry, &opts()).await.unwrap();
    assert_eq!(r.newly_failed, ["d1"]);
    assert_eq!(monitor.get("d1").unwrap().state, TrackedState::Failed);
    assert_eq!(client.removed(), [("d1".to_owned(), true)]);

    let history = db.history().for_movie(movie).await.unwrap();
    assert_eq!(history[0].event, HistoryEvent::DownloadFailed);
    assert_eq!(history[0].data["message"], "par2 repair failed");
    let blocked = db.blocklist().for_movie(movie).await.unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].indexer.as_deref(), Some("Idx"));
    assert_eq!(blocked[0].size, Some(4000));

    // A second pass does not record it again.
    let r = monitor.refresh(&db, &registry, &opts()).await.unwrap();
    assert!(r.newly_failed.is_empty());
    assert_eq!(db.blocklist().for_movie(movie).await.unwrap().len(), 1);
}

#[tokio::test]
async fn already_imported_downloads_stay_imported_after_a_restart() {
    let (db, client, registry, movie) = rig().await;
    grab_row(&db, movie, "d1").await;
    let mut imported = db.history().by_download_id("d1").await.unwrap().remove(0);
    imported.id = 0;
    imported.event = HistoryEvent::DownloadFolderImported;
    imported.date = now();
    db.history().insert(&imported).await.unwrap();

    client.set_items(vec![item("d1", DownloadItemStatus::Completed)]);
    let monitor = DownloadMonitor::new(); // fresh process
    let r = monitor.refresh(&db, &registry, &opts()).await.unwrap();
    assert_eq!(monitor.get("d1").unwrap().state, TrackedState::Imported);
    assert!(r.newly_pending.is_empty());
}

#[tokio::test]
async fn foreign_downloads_are_ignored_unless_in_our_category() {
    let (db, client, registry, _) = rig().await;
    let mut other = item("x", DownloadItemStatus::Completed);
    other.category = Some("tv".into());
    let mut ours = item("y", DownloadItemStatus::Completed);
    ours.category = Some("movies".into());
    client.set_items(vec![other, ours]);

    let monitor = DownloadMonitor::new();
    monitor.refresh(&db, &registry, &opts()).await.unwrap();
    assert!(monitor.get("x").is_none());
    let t = monitor.get("y").unwrap();
    assert_eq!(
        t.state,
        TrackedState::ImportBlocked,
        "no grab, so a person must pick the movie"
    );
    assert!(!t.warnings.is_empty());
}

#[tokio::test]
async fn removed_downloads_are_forgotten_but_an_unreachable_client_keeps_them() {
    let (db, client, registry, movie) = rig().await;
    grab_row(&db, movie, "d1").await;
    let monitor = DownloadMonitor::new();
    client.set_items(vec![item("d1", DownloadItemStatus::Downloading)]);
    monitor.refresh(&db, &registry, &opts()).await.unwrap();

    client.set_items(vec![]);
    monitor.refresh(&db, &registry, &opts()).await.unwrap();
    assert!(monitor.get("d1").is_none(), "gone from the client");

    client.set_items(vec![item("d1", DownloadItemStatus::Downloading)]);
    monitor.refresh(&db, &registry, &opts()).await.unwrap();
    client.fail_items(Some(ClientError::Unavailable("down".into())));
    let r = monitor.refresh(&db, &registry, &opts()).await.unwrap();
    assert_eq!(r.unreachable, ["SAB"]);
    assert!(
        monitor.get("d1").is_some(),
        "kept while the client cannot be asked"
    );
}
