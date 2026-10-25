use chrono::{DateTime, TimeZone, Utc};
use omp_core::{
    history::HistoryEvent,
    language::Language,
    movie::{MovieMetadata, MovieStatus},
    quality::Quality,
};
use omp_db::{Db, repo::NewMovie};
use omp_quality::profile::{ProfileItem, QualityProfile};
use omp_services::{rescan_movie, unmapped_folders};
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

struct Lib {
    db: Db,
    dir: tempfile::TempDir,
    movie: i64,
    movie_dir: PathBuf,
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

const PLAIN: &str = "The.Matrix.1999.1080p.BluRay.x264-GRP.mkv";

#[tokio::test]
async fn a_file_in_the_movie_folder_is_recorded_with_quality_from_its_name() {
    let l = lib().await;
    write(&l.movie_dir.join("extras/Making.Of.mkv"), 10);
    write(&l.movie_dir.join(PLAIN), 5000);
    write(
        &l.movie_dir
            .join("The.Matrix.1999.720p.WEB-DL.x264-OTHER.mkv"),
        9000,
    );
    write(&l.movie_dir.join("notes.txt"), 5);

    let r = rescan_movie(&l.db, l.movie, now()).await.unwrap();
    assert_eq!((r.added, r.removed, r.has_file), (true, false, true));
    let f =
        l.db.movie_files()
            .for_movie(l.movie)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(
        f.relative_path, PLAIN,
        "the better quality wins over the bigger file"
    );
    assert_eq!((f.quality.quality, f.size), (Quality::BLURAY_1080P, 5000));
    assert_eq!(f.release_group.as_deref(), Some("GRP"));

    // Nothing changes on a second pass.
    let again = rescan_movie(&l.db, l.movie, now()).await.unwrap();
    assert_eq!(
        (again.added, again.removed, again.has_file),
        (false, false, true)
    );
}

#[tokio::test]
async fn unknown_names_fall_back_to_the_extension_and_nested_files_are_found() {
    let l = lib().await;
    write(&l.movie_dir.join("CD1/feature.mkv"), 123);
    rescan_movie(&l.db, l.movie, now()).await.unwrap();
    let f =
        l.db.movie_files()
            .for_movie(l.movie)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(f.relative_path, "CD1/feature.mkv");
    assert_ne!(
        f.quality.quality,
        Quality::UNKNOWN,
        "the container says something"
    );
}

#[tokio::test]
async fn a_missing_file_is_forgotten_and_a_replacement_is_picked_up() {
    let l = lib().await;
    let first = write(&l.movie_dir.join(PLAIN), 100);
    rescan_movie(&l.db, l.movie, now()).await.unwrap();

    std::fs::remove_file(&first).unwrap();
    let r = rescan_movie(&l.db, l.movie, now()).await.unwrap();
    assert_eq!((r.removed, r.has_file), (true, false));
    assert!(
        l.db.movie_files()
            .for_movie(l.movie)
            .await
            .unwrap()
            .is_none()
    );
    let h = l.db.history().for_movie(l.movie).await.unwrap();
    assert_eq!(h[0].event, HistoryEvent::MovieFileDeleted);

    write(
        &l.movie_dir.join("The.Matrix.1999.720p.WEB-DL.x264-NEW.mkv"),
        50,
    );
    assert!(rescan_movie(&l.db, l.movie, now()).await.unwrap().added);
    // The recorded file vanishes and another takes its place in one scan.
    std::fs::remove_file(l.movie_dir.join("The.Matrix.1999.720p.WEB-DL.x264-NEW.mkv")).unwrap();
    write(&l.movie_dir.join(PLAIN), 70);
    let r = rescan_movie(&l.db, l.movie, now()).await.unwrap();
    assert!(r.added && r.has_file);
    assert_eq!(
        l.db.movie_files()
            .for_movie(l.movie)
            .await
            .unwrap()
            .unwrap()
            .relative_path,
        PLAIN
    );
}

#[tokio::test]
async fn a_folder_that_does_not_exist_is_not_an_error() {
    let l = lib().await;
    let r = rescan_movie(&l.db, l.movie, now()).await.unwrap();
    assert_eq!((r.added, r.removed, r.has_file), (false, false, false));
}

#[tokio::test]
async fn folders_not_in_the_library_are_listed_with_their_titles() {
    let l = lib().await;
    let root = l.dir.path().join("movies");
    write(&l.movie_dir.join(PLAIN), 1); // The Matrix is already in the library
    write(&root.join("Dune (2021)/dune.mkv"), 1);
    write(
        &root.join("Blade.Runner.2049.2017.1080p.BluRay.x264-GRP/x.mkv"),
        1,
    );
    std::fs::create_dir_all(root.join("Empty Folder")).unwrap();
    std::fs::create_dir_all(root.join(".hidden")).unwrap();
    write(&root.join("loose-file.mkv"), 1);

    let folders = unmapped_folders(&l.db, &root).await.unwrap();
    let names: Vec<_> = folders.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Blade.Runner.2049.2017.1080p.BluRay.x264-GRP",
            "Dune (2021)",
            "Empty Folder"
        ]
    );
    let dune = folders.iter().find(|f| f.name == "Dune (2021)").unwrap();
    assert_eq!(
        (dune.title.as_str(), dune.year, dune.has_media),
        ("Dune", Some(2021), true)
    );
    let blade = &folders[0];
    assert_eq!(
        (blade.title.as_str(), blade.year),
        ("Blade Runner 2049", Some(2017))
    );
    assert!(
        !folders
            .iter()
            .find(|f| f.name == "Empty Folder")
            .unwrap()
            .has_media
    );
}
