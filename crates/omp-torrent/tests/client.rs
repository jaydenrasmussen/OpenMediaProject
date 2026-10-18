//! Runs the real librqbit with no network: the torrent's data is already on
//! disk, so adding it hashes the files and it completes at once.

use std::time::Duration;

use axum::{Router, routing::get};
use chrono::Utc;
use librqbit::{CreateTorrentOptions, create_torrent, spawn_utils::BlockingSpawner};
use omp_core::release::{DownloadProtocol, IndexerFlags, ReleaseInfo};
use omp_downloads::{AddRequest, DownloadClient, DownloadItemStatus};
use omp_torrent::{TorrentClient, TorrentSettings};

fn release(url: String) -> ReleaseInfo {
    ReleaseInfo {
        guid: "g".into(),
        title: "Some.Movie.2020.1080p.BluRay.x264-GRP".into(),
        size: 0,
        download_url: url,
        indexer_id: 0,
        indexer: "Idx".into(),
        indexer_priority: 25,
        protocol: DownloadProtocol::Torrent,
        publish_date: Utc::now(),
        container: None,
        indexer_flags: IndexerFlags(0),
        imdb_id: None,
        tmdb_id: 0,
        torrent: None,
    }
}

async fn serve(bytes: Vec<u8>) -> String {
    let app = Router::new().route("/a.torrent", get(move || async move { bytes.clone() }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/a.torrent", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    url
}

async fn wait_for(client: &TorrentClient, status: DownloadItemStatus) -> omp_downloads::ClientItem {
    for _ in 0..100 {
        let items = client.items().await.unwrap();
        if let Some(i) = items.into_iter().find(|i| i.status == status) {
            return i;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!(
        "never reached {status:?}: {:?}",
        client.items().await.unwrap()
    );
}

#[tokio::test]
async fn adds_from_a_url_tracks_to_completion_and_removes() {
    let dir = tempfile::tempdir().unwrap();
    let downloads = dir.path().join("downloads");

    // The finished data, where the client will look for it: <dir>/movies/<name>/
    let data = downloads.join("movies").join("Some.Movie.2020");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("movie.mkv"), vec![7u8; 300_000]).unwrap();
    std::fs::write(data.join("movie.nfo"), b"info").unwrap();

    let made = create_torrent(
        &data,
        CreateTorrentOptions::default(),
        &BlockingSpawner::new(1),
    )
    .await
    .unwrap();
    let url = serve(made.as_bytes().unwrap().to_vec()).await;

    let mut settings = TorrentSettings::new(&downloads);
    settings.enable_dht = false;
    let client = TorrentClient::start(settings).await.unwrap();
    assert_eq!(client.protocol(), DownloadProtocol::Torrent);

    let rel = release(url);
    let id = client
        .add(&AddRequest {
            release: &rel,
            category: Some("movies"),
        })
        .await
        .unwrap();
    assert_eq!(id.len(), 40);
    assert_eq!(id, id.to_uppercase());
    assert_eq!(id, made.info_hash().as_string().to_uppercase());

    let item = wait_for(&client, DownloadItemStatus::Completed).await;
    assert_eq!(item.download_id, id);
    assert_eq!(item.category.as_deref(), Some("movies"));
    assert_eq!(item.title, "Some.Movie.2020");
    assert_eq!(item.remaining_size, 0);
    assert!(
        item.can_move_files && item.can_be_removed,
        "no seeding goal, so done"
    );
    let out = std::path::PathBuf::from(item.output_path.unwrap());
    assert!(
        out.join("movie.mkv").exists(),
        "output path points at the content: {out:?}"
    );

    // Adding it again is not an error and gives the same id.
    assert_eq!(
        client
            .add(&AddRequest {
                release: &rel,
                category: Some("movies")
            })
            .await
            .unwrap(),
        id
    );

    client.remove(&id, false).await.unwrap();
    assert!(client.items().await.unwrap().is_empty());
    assert!(
        out.join("movie.mkv").exists(),
        "data kept when not asked to delete it"
    );
    client.stop().await;
}

#[tokio::test]
async fn seeding_goal_keeps_files_in_place_until_met() {
    let dir = tempfile::tempdir().unwrap();
    let downloads = dir.path().join("downloads");
    let data = dir.path().join("source").join("Seeded");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("a.mkv"), vec![1u8; 100_000]).unwrap();
    // A torrent with a single file lands directly in the category folder.
    std::fs::create_dir_all(downloads.join("movies")).unwrap();
    std::fs::write(downloads.join("movies").join("a.mkv"), vec![1u8; 100_000]).unwrap();
    let made = create_torrent(
        &data,
        CreateTorrentOptions::default(),
        &BlockingSpawner::new(1),
    )
    .await
    .unwrap();
    let url = serve(made.as_bytes().unwrap().to_vec()).await;

    let mut settings = TorrentSettings::new(&downloads);
    settings.enable_dht = false;
    settings.seed_ratio_limit = 2.0;
    let client = TorrentClient::start(settings).await.unwrap();
    client
        .add(&AddRequest {
            release: &release(url),
            category: Some("movies"),
        })
        .await
        .unwrap();

    let item = wait_for(&client, DownloadItemStatus::Completed).await;
    assert!(
        item.output_path
            .as_deref()
            .unwrap()
            .ends_with("movies/a.mkv"),
        "{:?}",
        item.output_path
    );
    assert!(
        !item.can_move_files && !item.can_be_removed,
        "still has to seed to ratio 2"
    );
    client.stop().await;
}

#[tokio::test]
async fn bad_sources_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = TorrentSettings::new(dir.path());
    settings.enable_dht = false;
    let client = TorrentClient::start(settings).await.unwrap();

    let none = release(String::new());
    let r = client
        .add(&AddRequest {
            release: &none,
            category: None,
        })
        .await;
    assert!(
        matches!(r, Err(omp_downloads::ClientError::ReleaseUnavailable(_))),
        "{r:?}"
    );

    let dead = release("http://127.0.0.1:9/nothing.torrent".into());
    let r = client
        .add(&AddRequest {
            release: &dead,
            category: None,
        })
        .await;
    assert!(
        matches!(
            r,
            Err(omp_downloads::ClientError::ReleaseDownloadFailed { .. })
        ),
        "{r:?}"
    );
    client.stop().await;
}
