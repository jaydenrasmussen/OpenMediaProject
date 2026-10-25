use async_trait::async_trait;
use chrono::{DateTime, Duration, TimeZone, Utc};
use omp_core::{
    history::HistoryEvent,
    language::Language,
    movie::{MovieMetadata, MovieStatus},
    quality::Quality,
    release::IndexerFlags,
};
use omp_db::{Db, repo::NewMovie};
use omp_quality::profile::{ProfileItem, QualityProfile};
use omp_services::{
    FileOutcome, ImportConfig, ImportMode, ImportRejection, ImportRequest, Importer, MediaProbe,
    NoDiskCheck, ProbeInfo,
};
use std::path::{Path, PathBuf};

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

struct FakeProbe {
    runtime: f64,
    audio: u32,
    unreadable: bool,
}

#[async_trait]
impl MediaProbe for FakeProbe {
    async fn probe(&self, _path: &Path) -> Option<ProbeInfo> {
        (!self.unreadable).then(|| ProbeInfo {
            runtime_seconds: self.runtime,
            audio_streams: self.audio,
            raw: serde_json::json!({"fake": true}),
        })
    }
}

const GOOD: FakeProbe = FakeProbe {
    runtime: 7200.0,
    audio: 1,
    unreadable: false,
};

struct Lib {
    db: Db,
    dir: tempfile::TempDir,
    movie: i64,
    movie_dir: PathBuf,
}

impl Lib {
    fn downloads(&self) -> PathBuf {
        self.dir.path().join("downloads")
    }
}

async fn lib() -> Lib {
    let db = setup().await;
    let movie = add_movie(&db, 603, "The Matrix", 1999).await;
    let dir = tempfile::tempdir().unwrap();
    let movie_dir = dir.path().join("movies/The Matrix (1999)");
    let m = db.movies().get(movie).await.unwrap();
    db.movies()
        .update(
            movie,
            &omp_db::repo::MovieUpdate {
                monitored: true,
                minimum_availability: m.movie.minimum_availability,
                quality_profile_id: m.movie.quality_profile_id,
                path: movie_dir.to_string_lossy().into_owned(),
                root_folder_path: None,
                tags: vec![],
            },
        )
        .await
        .unwrap();
    std::fs::create_dir_all(dir.path().join("downloads")).unwrap();
    Lib {
        db,
        dir,
        movie,
        movie_dir,
    }
}

fn write(path: &Path, bytes: usize) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![b'x'; bytes]).unwrap();
    path.to_path_buf()
}

fn config() -> ImportConfig {
    ImportConfig {
        naming: omp_naming::NamingConfig {
            rename_movies: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

async fn run(
    l: &Lib,
    probe: &FakeProbe,
    cfg: &ImportConfig,
    output: &Path,
    can_move: bool,
) -> omp_services::ImportResult {
    Importer {
        db: &l.db,
        config: cfg,
        probe,
        disk: &NoDiskCheck,
    }
    .import(&ImportRequest {
        movie_id: l.movie,
        output_path: output,
        download_id: Some("d1"),
        download_title: "The.Matrix.1999.1080p.BluRay.x264-GRP",
        can_move_files: can_move,
        now: now(),
        manual: None,
    })
    .await
    .unwrap()
}

fn rejections(r: &omp_services::ImportResult, i: usize) -> &[ImportRejection] {
    match &r.files[i].outcome {
        FileOutcome::Rejected(v) => v,
        other => panic!("expected rejection, got {other:?}"),
    }
}

#[tokio::test]
async fn a_finished_download_is_renamed_moved_and_recorded() {
    let l = lib().await;
    let out = l.downloads().join("The.Matrix.1999.1080p.BluRay.x264-GRP");
    let src = write(&out.join("the.matrix.1999.1080p.bluray.x264-grp.mkv"), 2000);
    write(&out.join("readme.nfo"), 10);

    let r = run(&l, &GOOD, &config(), &out, true).await;
    assert!(r.imported(), "{r:?}");
    let dest = l.movie_dir.join("The Matrix (1999) Bluray-1080p.mkv");
    assert!(dest.exists(), "renamed into the movie folder");
    assert!(!src.exists(), "moved, not copied");

    let file =
        l.db.movie_files()
            .for_movie(l.movie)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(file.relative_path, "The Matrix (1999) Bluray-1080p.mkv");
    assert_eq!(file.size, 2000);
    assert_eq!(file.quality.quality, Quality::BLURAY_1080P);
    assert_eq!(
        file.release_group.as_deref(),
        Some("grp"),
        "taken from the file name as written"
    );
    assert!(file.media_info.is_some());
    let h = l.db.history().for_movie(l.movie).await.unwrap();
    assert_eq!(h[0].event, HistoryEvent::DownloadFolderImported);
    assert_eq!(h[0].download_id.as_deref(), Some("d1"));
}

#[tokio::test]
async fn files_the_client_still_needs_are_kept() {
    let l = lib().await;
    let src = write(
        &l.downloads()
            .join("The.Matrix.1999.1080p.BluRay.x264-GRP.mkv"),
        500,
    );
    let r = run(&l, &GOOD, &config(), &src, false).await;
    assert!(r.imported());
    assert!(src.exists(), "a seeding torrent keeps its file");
    assert!(
        l.movie_dir
            .join("The Matrix (1999) Bluray-1080p.mkv")
            .exists()
    );

    // Forced move overrides it.
    let l = lib().await;
    let src = write(
        &l.downloads()
            .join("The.Matrix.1999.1080p.BluRay.x264-GRP.mkv"),
        500,
    );
    let cfg = ImportConfig {
        mode: ImportMode::Move,
        ..config()
    };
    assert!(run(&l, &GOOD, &cfg, &src, false).await.imported());
    assert!(!src.exists());
}

#[tokio::test]
async fn samples_silent_files_and_unreadable_files_are_rejected() {
    let l = lib().await;
    let src = write(
        &l.downloads()
            .join("The.Matrix.1999.1080p.BluRay.x264-GRP.mkv"),
        500,
    );

    let sample = FakeProbe {
        runtime: 30.0,
        ..GOOD
    };
    assert_eq!(
        rejections(&run(&l, &sample, &config(), &src, true).await, 0),
        [ImportRejection::Sample]
    );
    let silent = FakeProbe { audio: 0, ..GOOD };
    assert_eq!(
        rejections(&run(&l, &silent, &config(), &src, true).await, 0),
        [ImportRejection::NoAudio]
    );
    let unreadable = FakeProbe {
        unreadable: true,
        ..GOOD
    };
    assert_eq!(
        rejections(&run(&l, &unreadable, &config(), &src, true).await, 0),
        [ImportRejection::SampleIndeterminate]
    );
    assert!(src.exists(), "rejected files are left alone");
    assert!(
        l.db.movie_files()
            .for_movie(l.movie)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn only_the_best_file_of_a_folder_is_imported() {
    let l = lib().await;
    let out = l.downloads().join("pack");
    let small = write(&out.join("The.Matrix.1999.720p.WEB-DL.x264-GRP.mkv"), 100);
    let big = write(&out.join("The.Matrix.1999.1080p.BluRay.x264-GRP.mkv"), 300);
    let r = run(&l, &GOOD, &config(), &out, true).await;
    assert_eq!(
        r.files
            .iter()
            .filter(|f| matches!(f.outcome, FileOutcome::Imported { .. }))
            .count(),
        1
    );
    assert!(!big.exists() && small.exists());
    assert_eq!(
        l.db.movie_files()
            .for_movie(l.movie)
            .await
            .unwrap()
            .unwrap()
            .quality
            .quality,
        Quality::BLURAY_1080P
    );
    let other = r.files.iter().find(|f| f.path == small).unwrap();
    assert!(
        matches!(&other.outcome, FileOutcome::Rejected(v) if v == &[ImportRejection::AlreadyImportedThisRun])
    );
}

#[tokio::test]
async fn an_upgrade_replaces_the_old_file_and_a_downgrade_is_refused() {
    let l = lib().await;
    let old = write(&l.movie_dir.join("The Matrix (1999) WEBDL-720p.mkv"), 50);
    l.db.movie_files()
        .set(&omp_db::repo::MovieFileRecord {
            id: 0,
            movie_id: l.movie,
            relative_path: "The Matrix (1999) WEBDL-720p.mkv".into(),
            size: 50,
            date_added: now() - Duration::days(30),
            scene_name: None,
            release_group: None,
            edition: None,
            quality: omp_core::quality::QualityModel {
                quality: Quality::WEBDL_720P,
                ..Default::default()
            },
            languages: vec![],
            media_info: None,
            original_file_path: None,
            indexer_flags: IndexerFlags(0),
        })
        .await
        .unwrap();

    let worse = write(
        &l.downloads().join("The.Matrix.1999.DVDRip.x264-GRP.mkv"),
        500,
    );
    let r = run(&l, &GOOD, &config(), &worse, true).await;
    assert!(matches!(
        rejections(&r, 0),
        [ImportRejection::NotQualityUpgrade(_)]
    ));
    assert!(old.exists() && worse.exists());

    let better = write(
        &l.downloads()
            .join("The.Matrix.1999.1080p.BluRay.x264-GRP.mkv"),
        900,
    );
    let r = run(&l, &GOOD, &config(), &better, true).await;
    assert!(r.imported(), "{r:?}");
    assert!(
        !old.exists(),
        "old file removed after the new one is in place"
    );
    assert!(
        l.movie_dir
            .join("The Matrix (1999) Bluray-1080p.mkv")
            .exists()
    );
    let file =
        l.db.movie_files()
            .for_movie(l.movie)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(file.quality.quality, Quality::BLURAY_1080P);
}

#[tokio::test]
async fn multi_part_files_and_unpack_folders_are_rejected() {
    let l = lib().await;
    let out = l.downloads().join("multi");
    let cd1 = write(
        &out.join("The.Matrix.1999.1080p.BluRay.x264-GRP.cd1.mkv"),
        100,
    );
    write(
        &out.join("The.Matrix.1999.1080p.BluRay.x264-GRP.cd2.mkv"),
        100,
    );
    let r = run(&l, &GOOD, &config(), &out, true).await;
    assert!(!r.imported());
    assert!(r.files.iter().all(|f| matches!(&f.outcome, FileOutcome::Rejected(v) if v.contains(&ImportRejection::MultiPart))));
    assert!(cd1.exists());

    let unpack = write(
        &l.downloads()
            .join("_UNPACK_x/The.Matrix.1999.1080p.BluRay.x264-GRP.mkv"),
        100,
    );
    let r = run(&l, &GOOD, &config(), &unpack, true).await;
    assert!(rejections(&r, 0).contains(&ImportRejection::Unpacking));
}

#[tokio::test]
async fn a_download_already_imported_is_not_imported_twice() {
    let l = lib().await;
    let src = write(
        &l.downloads()
            .join("The.Matrix.1999.1080p.BluRay.x264-GRP.mkv"),
        100,
    );
    grab_and_import_marker(&l).await;
    // The movie has a file and this download id was imported after its grab.
    let r = run(&l, &GOOD, &config(), &src, true).await;
    assert!(
        matches!(
            rejections(&r, 0).first(),
            Some(ImportRejection::AlreadyImported(_))
        ),
        "{r:?}"
    );
}

async fn grab_and_import_marker(l: &Lib) {
    let rec = |event, date| omp_db::repo::HistoryRecord {
        id: 0,
        movie_id: l.movie,
        event,
        date,
        source_title: "x".into(),
        quality: Default::default(),
        languages: vec![],
        download_id: Some("d1".into()),
        data: Default::default(),
    };
    l.db.history()
        .insert(&rec(HistoryEvent::Grabbed, now() - Duration::hours(2)))
        .await
        .unwrap();
    l.db.history()
        .insert(&rec(
            HistoryEvent::DownloadFolderImported,
            now() - Duration::hours(1),
        ))
        .await
        .unwrap();
    l.db.movie_files()
        .set(&omp_db::repo::MovieFileRecord {
            id: 0,
            movie_id: l.movie,
            relative_path: "a.mkv".into(),
            size: 1,
            date_added: now(),
            scene_name: None,
            release_group: None,
            edition: None,
            quality: omp_core::quality::QualityModel {
                quality: Quality::BLURAY_1080P,
                ..Default::default()
            },
            languages: vec![],
            media_info: None,
            original_file_path: None,
            indexer_flags: IndexerFlags(0),
        })
        .await
        .unwrap();
}

// ---- the download loop ---------------------------------------------------------------------

mod process {
    use super::*;
    use omp_downloads::{
        ClientItem, ClientRegistry, DownloadItemStatus, FakeClient, RegisteredClient,
    };
    use omp_services::{DownloadMonitor, ProcessOptions, TrackedState, process_downloads};
    use std::sync::Arc;

    fn item(l: &Lib, status: DownloadItemStatus, folder: &str) -> ClientItem {
        ClientItem {
            download_id: "d1".into(),
            category: Some("movies".into()),
            title: "The.Matrix.1999.1080p.BluRay.x264-GRP".into(),
            total_size: 100,
            remaining_size: 0,
            status,
            output_path: Some(l.downloads().join(folder).to_string_lossy().into_owned()),
            seed_ratio: None,
            can_move_files: true,
            can_be_removed: true,
            message: None,
            ..Default::default()
        }
    }

    async fn rig(l: &Lib) -> (Arc<FakeClient>, ClientRegistry) {
        let client = Arc::new(FakeClient::new(omp_core::release::DownloadProtocol::Usenet));
        let registry = ClientRegistry::new(vec![RegisteredClient {
            id: 1,
            name: "SAB".into(),
            priority: 1,
            tags: vec![],
            client: client.clone(),
        }]);
        let grab = omp_db::repo::HistoryRecord {
            id: 0,
            movie_id: l.movie,
            event: HistoryEvent::Grabbed,
            date: now() - Duration::hours(1),
            source_title: "The.Matrix.1999.1080p.BluRay.x264-GRP".into(),
            quality: Default::default(),
            languages: vec![],
            download_id: Some("d1".into()),
            data: Default::default(),
        };
        l.db.history().insert(&grab).await.unwrap();
        (client, registry)
    }

    fn opts() -> ProcessOptions {
        ProcessOptions {
            now: now(),
            category: Some("movies".into()),
            remove_failed: true,
            remove_completed: true,
        }
    }

    #[tokio::test]
    async fn finished_downloads_are_imported_and_removed_from_the_client() {
        let l = lib().await;
        let (client, registry) = rig(&l).await;
        write(
            &l.downloads()
                .join("job/The.Matrix.1999.1080p.BluRay.x264-GRP.mkv"),
            800,
        );
        let monitor = DownloadMonitor::new();

        client.set_items(vec![item(&l, DownloadItemStatus::Downloading, "job")]);
        let r = process_downloads(
            &l.db,
            &monitor,
            &registry,
            &config(),
            &GOOD,
            &NoDiskCheck,
            &opts(),
        )
        .await
        .unwrap();
        assert!(r.imported.is_empty());

        client.set_items(vec![item(&l, DownloadItemStatus::Completed, "job")]);
        let r = process_downloads(
            &l.db,
            &monitor,
            &registry,
            &config(),
            &GOOD,
            &NoDiskCheck,
            &opts(),
        )
        .await
        .unwrap();
        assert_eq!(r.imported, ["d1"]);
        assert_eq!(monitor.get("d1").unwrap().state, TrackedState::Imported);
        assert_eq!(client.removed(), [("d1".to_owned(), false)]);
        assert!(
            l.movie_dir
                .join("The Matrix (1999) Bluray-1080p.mkv")
                .exists()
        );

        // Next pass: nothing left to do.
        let r = process_downloads(
            &l.db,
            &monitor,
            &registry,
            &config(),
            &GOOD,
            &NoDiskCheck,
            &opts(),
        )
        .await
        .unwrap();
        assert!(r.imported.is_empty() && r.blocked.is_empty());
    }

    #[tokio::test]
    async fn a_download_that_cannot_be_imported_is_blocked_with_reasons() {
        let l = lib().await;
        let (client, registry) = rig(&l).await;
        write(
            &l.downloads()
                .join("job/The.Matrix.1999.1080p.BluRay.x264-GRP.mkv"),
            800,
        );
        let monitor = DownloadMonitor::new();
        client.set_items(vec![item(&l, DownloadItemStatus::Completed, "job")]);

        let sample = FakeProbe {
            runtime: 20.0,
            ..GOOD
        };
        let r = process_downloads(
            &l.db,
            &monitor,
            &registry,
            &config(),
            &sample,
            &NoDiskCheck,
            &opts(),
        )
        .await
        .unwrap();
        assert!(r.imported.is_empty());
        assert_eq!(r.blocked.len(), 1);
        assert!(r.blocked[0].1[0].contains("Sample"), "{:?}", r.blocked);
        assert_eq!(
            monitor.get("d1").unwrap().state,
            TrackedState::ImportBlocked
        );
        assert!(
            client.removed().is_empty(),
            "left in the client for a person to sort out"
        );

        // An empty output folder is reported too.
        std::fs::create_dir_all(l.downloads().join("empty")).unwrap();
        let monitor = DownloadMonitor::new();
        client.set_items(vec![item(&l, DownloadItemStatus::Completed, "empty")]);
        let r = process_downloads(
            &l.db,
            &monitor,
            &registry,
            &config(),
            &GOOD,
            &NoDiskCheck,
            &opts(),
        )
        .await
        .unwrap();
        assert!(r.blocked[0].1[0].contains("No files found"));
    }
}
