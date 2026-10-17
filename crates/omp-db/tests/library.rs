use std::collections::BTreeMap;

use chrono::{DateTime, Duration, TimeZone, Utc};
use omp_core::{
    blocklist::{BlocklistEntry, is_blocklisted},
    history::HistoryEvent,
    language::Language,
    movie::{CoverKind, Credit, CreditKind, MediaCover, MovieMetadata, MovieStatus, Rating},
    quality::{Quality, QualityModel},
    release::{DownloadProtocol, IndexerFlags, ReleaseInfo},
};
use omp_db::{
    Db, DbError,
    repo::{
        HistoryRecord, MovieFileRecord, MovieUpdate, NewMovie, PendingReason, Provider,
        ProviderKind,
    },
};
use omp_quality::profile::{ProfileItem, QualityProfile};

fn t(days: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap() + Duration::days(days)
}

fn metadata(tmdb_id: i64, title: &str) -> MovieMetadata {
    MovieMetadata {
        tmdb_id,
        imdb_id: Some(format!("tt{tmdb_id:07}")),
        title: title.into(),
        original_title: title.into(),
        original_language: "en".into(),
        overview: Some("An overview.".into()),
        year: 1999,
        runtime: 136,
        status: MovieStatus::Released,
        certification: Some("R".into()),
        genres: vec!["Action".into(), "Science Fiction".into()],
        in_cinemas: Some(t(0)),
        physical_release: Some(t(200)),
        digital_release: None,
        collection_tmdb_id: Some(2344),
        collection_title: Some("The Matrix Collection".into()),
        ratings: Some(Rating {
            votes: 100,
            value: 8.2,
        }),
        images: vec![MediaCover {
            kind: CoverKind::Poster,
            url: "https://image.example/p.jpg".into(),
        }],
        credits: vec![Credit {
            person_tmdb_id: 6384,
            name: "Keanu Reeves".into(),
            kind: CreditKind::Cast,
            role: "Neo".into(),
            order: 0,
            image_url: None,
        }],
        popularity: 49.5,
        ..Default::default()
    }
}

async fn profile_id(db: &Db) -> i32 {
    db.quality_profiles()
        .insert(&QualityProfile {
            id: 0,
            name: "HD".into(),
            cutoff: Quality::WEBDL_1080P.id,
            items: vec![ProfileItem::Quality {
                quality: Quality::WEBDL_1080P,
                allowed: true,
            }],
            min_format_score: 0,
            cutoff_format_score: 0,
            min_upgrade_format_score: 1,
            format_items: vec![],
            language: Language::ANY,
            upgrade_allowed: true,
        })
        .await
        .unwrap()
}

async fn add_movie(db: &Db, tmdb_id: i64, title: &str, profile: i32) -> i64 {
    let metadata_id = db
        .movies()
        .upsert_metadata(&metadata(tmdb_id, title))
        .await
        .unwrap();
    db.movies()
        .add(&NewMovie {
            metadata_id,
            monitored: true,
            minimum_availability: MovieStatus::Released,
            quality_profile_id: profile,
            path: format!("/movies/{title}"),
            root_folder_path: Some("/movies".into()),
            tags: vec![1, 2],
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn metadata_upsert_is_keyed_by_tmdb_id() {
    let db = Db::in_memory().await.unwrap();
    let first = db
        .movies()
        .upsert_metadata(&metadata(603, "The Matrix"))
        .await
        .unwrap();
    let mut changed = metadata(603, "The Matrix (Remastered)");
    changed.runtime = 140;
    let second = db.movies().upsert_metadata(&changed).await.unwrap();
    assert_eq!(first, second, "same TMDB id, same row");
    let other = db
        .movies()
        .upsert_metadata(&metadata(604, "Reloaded"))
        .await
        .unwrap();
    assert_ne!(first, other);
}

#[tokio::test]
async fn a_movie_round_trips_with_its_metadata() {
    let db = Db::in_memory().await.unwrap();
    let profile = profile_id(&db).await;
    let id = add_movie(&db, 603, "The Matrix", profile).await;

    let record = db.movies().get(id).await.unwrap();
    assert_eq!(record.movie.id, id);
    assert_eq!(record.movie.tmdb_id, 603);
    assert_eq!(record.movie.title, "The Matrix");
    assert_eq!(record.movie.tags, [1, 2]);
    assert!(record.movie.monitored);
    assert_eq!(record.movie.minimum_availability, MovieStatus::Released);
    assert_eq!(record.root_folder_path.as_deref(), Some("/movies"));
    // Everything the provider gave us survives the trip.
    assert_eq!(record.metadata, metadata(603, "The Matrix"));

    let by_tmdb = db.movies().find_by_tmdb_id(603).await.unwrap().unwrap();
    assert_eq!(by_tmdb.movie.id, id);
    assert!(db.movies().find_by_tmdb_id(1).await.unwrap().is_none());
    assert!(matches!(db.movies().get(999).await, Err(DbError::NotFound)));
}

#[tokio::test]
async fn adding_the_same_movie_twice_conflicts_and_profiles_must_exist() {
    let db = Db::in_memory().await.unwrap();
    let profile = profile_id(&db).await;
    let metadata_id = db
        .movies()
        .upsert_metadata(&metadata(603, "The Matrix"))
        .await
        .unwrap();
    let new = NewMovie {
        metadata_id,
        monitored: true,
        minimum_availability: MovieStatus::Released,
        quality_profile_id: profile,
        path: "/m".into(),
        root_folder_path: None,
        tags: vec![],
    };
    db.movies().add(&new).await.unwrap();
    assert!(matches!(
        db.movies().add(&new).await,
        Err(DbError::Conflict(_))
    ));

    let other = db
        .movies()
        .upsert_metadata(&metadata(700, "Other"))
        .await
        .unwrap();
    let bad_profile = NewMovie {
        metadata_id: other,
        quality_profile_id: 9999,
        ..new
    };
    assert!(matches!(
        db.movies().add(&bad_profile).await,
        Err(DbError::Constraint(_))
    ));
}

#[tokio::test]
async fn profiles_in_use_cannot_be_deleted() {
    let db = Db::in_memory().await.unwrap();
    let profile = profile_id(&db).await;
    add_movie(&db, 603, "The Matrix", profile).await;
    assert!(matches!(
        db.quality_profiles().delete(profile).await,
        Err(DbError::Constraint(_))
    ));
}

#[tokio::test]
async fn list_find_by_title_update_and_search_time() {
    let db = Db::in_memory().await.unwrap();
    let profile = profile_id(&db).await;
    let b = add_movie(&db, 2, "Blade Runner", profile).await;
    let a = add_movie(&db, 1, "The Abyss", profile).await;

    let titles: Vec<_> = db
        .movies()
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|m| m.movie.title)
        .collect();
    assert_eq!(
        titles,
        ["The Abyss", "Blade Runner"],
        "sorted ignoring the leading article"
    );

    let found = db
        .movies()
        .find_by_clean_title("bladerunner")
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].movie.id, b);

    db.movies()
        .update(
            a,
            &MovieUpdate {
                monitored: false,
                minimum_availability: MovieStatus::InCinemas,
                quality_profile_id: profile,
                path: "/elsewhere".into(),
                root_folder_path: None,
                tags: vec![9],
            },
        )
        .await
        .unwrap();
    let updated = db.movies().get(a).await.unwrap();
    assert!(!updated.movie.monitored);
    assert_eq!(updated.movie.path, "/elsewhere");
    assert_eq!(updated.movie.tags, [9]);

    db.movies().set_last_search_time(a, t(5)).await.unwrap();
    assert_eq!(
        db.movies().get(a).await.unwrap().last_search_time,
        Some(t(5))
    );
    assert!(matches!(
        db.movies()
            .update(
                999,
                &MovieUpdate {
                    monitored: true,
                    minimum_availability: MovieStatus::Released,
                    quality_profile_id: profile,
                    path: String::new(),
                    root_folder_path: None,
                    tags: vec![]
                }
            )
            .await,
        Err(DbError::NotFound)
    ));
}

fn file(movie_id: i64, name: &str) -> MovieFileRecord {
    MovieFileRecord {
        id: 0,
        movie_id,
        relative_path: name.into(),
        size: 4_000_000_000,
        date_added: t(1),
        scene_name: Some("The.Matrix.1999.1080p.WEB-DL-GRP".into()),
        release_group: Some("GRP".into()),
        edition: None,
        quality: QualityModel {
            quality: Quality::WEBDL_1080P,
            ..Default::default()
        },
        languages: vec![Language::ENGLISH, Language::GERMAN],
        media_info: Some(serde_json::json!({"video": {"codec": "h264"}})),
        original_file_path: Some("/downloads/x.mkv".into()),
        indexer_flags: IndexerFlags::G_FREELEECH,
    }
}

#[tokio::test]
async fn a_movie_has_at_most_one_file_and_it_converts_for_decisions() {
    let db = Db::in_memory().await.unwrap();
    let profile = profile_id(&db).await;
    let id = add_movie(&db, 603, "The Matrix", profile).await;
    assert!(db.movie_files().for_movie(id).await.unwrap().is_none());

    db.movie_files().set(&file(id, "first.mkv")).await.unwrap();
    let new_id = db.movie_files().set(&file(id, "second.mkv")).await.unwrap();
    let stored = db.movie_files().for_movie(id).await.unwrap().unwrap();
    assert_eq!(stored.id, new_id);
    assert_eq!(stored.relative_path, "second.mkv", "second replaced first");
    assert_eq!(stored.languages, [Language::ENGLISH, Language::GERMAN]);
    assert_eq!(stored.indexer_flags, IndexerFlags::G_FREELEECH);
    assert_eq!(stored.media_info, file(id, "x").media_info);

    let existing = stored.to_existing_file(vec![4, 5]);
    assert_eq!(existing.quality.quality, Quality::WEBDL_1080P);
    assert_eq!(existing.release_group.as_deref(), Some("GRP"));
    assert_eq!(existing.custom_format_ids, [4, 5]);

    db.movie_files().delete_for_movie(id).await.unwrap();
    assert!(db.movie_files().for_movie(id).await.unwrap().is_none());
}

fn history(movie_id: i64, event: HistoryEvent, day: i64, download_id: &str) -> HistoryRecord {
    HistoryRecord {
        id: 0,
        movie_id,
        event,
        date: t(day),
        source_title: "The.Matrix.1999.1080p.WEB-DL-GRP".into(),
        quality: QualityModel {
            quality: Quality::WEBDL_1080P,
            ..Default::default()
        },
        languages: vec![Language::ENGLISH],
        download_id: Some(download_id.into()),
        data: BTreeMap::from([("indexer".to_string(), "Idx".to_string())]),
    }
}

#[tokio::test]
async fn history_is_newest_first_and_joins_by_download_id() {
    let db = Db::in_memory().await.unwrap();
    let profile = profile_id(&db).await;
    let id = add_movie(&db, 603, "The Matrix", profile).await;
    db.history()
        .insert(&history(id, HistoryEvent::Grabbed, 1, "ABC"))
        .await
        .unwrap();
    db.history()
        .insert(&history(id, HistoryEvent::DownloadFolderImported, 2, "ABC"))
        .await
        .unwrap();
    db.history()
        .insert(&history(id, HistoryEvent::Grabbed, 3, "DEF"))
        .await
        .unwrap();

    let all = db.history().for_movie(id).await.unwrap();
    let days: Vec<_> = all.iter().map(|h| h.date).collect();
    assert_eq!(days, [t(3), t(2), t(1)]);
    assert_eq!(all[0].data["indexer"], "Idx");
    assert_eq!(all[1].event, HistoryEvent::DownloadFolderImported);

    let abc = db.history().by_download_id("ABC").await.unwrap();
    assert_eq!(
        abc.iter().map(|h| h.event).collect::<Vec<_>>(),
        [HistoryEvent::Grabbed, HistoryEvent::DownloadFolderImported]
    );
    assert_eq!(
        db.history()
            .most_recent_for_movie(id)
            .await
            .unwrap()
            .unwrap()
            .download_id
            .as_deref(),
        Some("DEF")
    );
}

fn release(title: &str) -> ReleaseInfo {
    ReleaseInfo {
        guid: "g".into(),
        title: title.into(),
        size: 4_000_000_000,
        download_url: "https://x".into(),
        indexer_id: 1,
        indexer: "Idx".into(),
        indexer_priority: 25,
        protocol: DownloadProtocol::Usenet,
        publish_date: t(3),
        container: None,
        indexer_flags: IndexerFlags::NONE,
        imdb_id: None,
        tmdb_id: 603,
        torrent: None,
    }
}

#[tokio::test]
async fn blocklist_round_trips_and_feeds_the_matcher() {
    let db = Db::in_memory().await.unwrap();
    let profile = profile_id(&db).await;
    let id = add_movie(&db, 603, "The Matrix", profile).await;
    let r = release("The.Matrix.1999.1080p.WEB-DL-GRP");
    let entry = BlocklistEntry {
        id: 0,
        movie_id: id,
        source_title: r.title.clone(),
        quality: QualityModel {
            quality: Quality::WEBDL_1080P,
            ..Default::default()
        },
        languages: vec![Language::ENGLISH],
        published_date: Some(r.publish_date),
        size: Some(r.size),
        protocol: DownloadProtocol::Usenet,
        indexer: Some("Idx".into()),
        indexer_flags: IndexerFlags::NONE,
        torrent_info_hash: None,
        message: Some("failed".into()),
        date: t(4),
    };
    let entry_id = db.blocklist().insert(&entry).await.unwrap();

    let stored = db.blocklist().for_movie(id).await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].id, entry_id);
    assert_eq!(stored[0].message.as_deref(), Some("failed"));
    assert!(is_blocklisted(&stored, &r));
    assert!(!is_blocklisted(&stored, &release("Another.Release")));

    db.blocklist().delete(entry_id).await.unwrap();
    assert!(db.blocklist().for_movie(id).await.unwrap().is_empty());
    assert!(matches!(
        db.blocklist().delete(entry_id).await,
        Err(DbError::NotFound)
    ));
}

#[tokio::test]
async fn pending_releases_round_trip_the_release() {
    let db = Db::in_memory().await.unwrap();
    let profile = profile_id(&db).await;
    let id = add_movie(&db, 603, "The Matrix", profile).await;
    let r = release("The.Matrix.1999.1080p.WEB-DL-GRP");
    let pending_id = db
        .pending_releases()
        .add(id, &r, PendingReason::Delay)
        .await
        .unwrap();

    let all = db.pending_releases().for_movie(id).await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].release, r);
    assert_eq!(all[0].reason, PendingReason::Delay);
    assert_eq!(db.pending_releases().list().await.unwrap().len(), 1);
    db.pending_releases().remove(pending_id).await.unwrap();
    assert!(db.pending_releases().list().await.unwrap().is_empty());
}

#[tokio::test]
async fn deleting_a_movie_removes_everything_that_hangs_off_it() {
    let db = Db::in_memory().await.unwrap();
    let profile = profile_id(&db).await;
    let id = add_movie(&db, 603, "The Matrix", profile).await;
    db.movie_files().set(&file(id, "x.mkv")).await.unwrap();
    db.history()
        .insert(&history(id, HistoryEvent::Grabbed, 1, "ABC"))
        .await
        .unwrap();
    db.pending_releases()
        .add(id, &release("x"), PendingReason::Delay)
        .await
        .unwrap();
    db.blocklist()
        .insert(&BlocklistEntry {
            id: 0,
            movie_id: id,
            source_title: "x".into(),
            quality: QualityModel::default(),
            languages: vec![],
            published_date: None,
            size: None,
            protocol: DownloadProtocol::Usenet,
            indexer: None,
            indexer_flags: IndexerFlags::NONE,
            torrent_info_hash: None,
            message: None,
            date: t(1),
        })
        .await
        .unwrap();

    db.movies().delete(id).await.unwrap();

    for table in ["movie_files", "history", "pending_releases", "blocklist"] {
        let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(db.main_pool())
            .await
            .unwrap();
        assert_eq!(n, 0, "{table} was not cleaned up");
    }
    // Provider metadata lingers until purged.
    assert_eq!(db.movies().purge_unused_metadata().await.unwrap(), 1);
    assert_eq!(db.movies().purge_unused_metadata().await.unwrap(), 0);
}

fn indexer(name: &str) -> Provider {
    Provider {
        id: 0,
        kind: ProviderKind::Indexer,
        name: name.into(),
        implementation: "newznab".into(),
        plugin_id: None,
        settings: serde_json::json!({"baseUrl": "https://idx.example", "apiKey": "k"}),
        tags: vec![3],
        enabled: true,
        enable_rss: true,
        enable_automatic_search: true,
        enable_interactive_search: false,
        priority: 10,
    }
}

#[tokio::test]
async fn providers_crud_and_uniqueness() {
    let db = Db::in_memory().await.unwrap();
    let repo = db.providers();
    let id = repo.insert(&indexer("NZBgeek")).await.unwrap();
    assert!(matches!(
        repo.insert(&indexer("NZBgeek")).await,
        Err(DbError::Conflict(_))
    ));

    // The same name under another kind is fine.
    let mut client = indexer("NZBgeek");
    client.kind = ProviderKind::DownloadClient;
    repo.insert(&client).await.unwrap();

    let mut stored = repo.get(id).await.unwrap();
    assert_eq!(stored.settings["apiKey"], "k");
    assert_eq!(stored.tags, [3]);
    stored.enabled = false;
    stored.priority = 1;
    repo.update(&stored).await.unwrap();
    assert!(!repo.get(id).await.unwrap().enabled);

    let mut second = indexer("Another");
    second.priority = 5;
    repo.insert(&second).await.unwrap();
    let names: Vec<_> = repo
        .list(ProviderKind::Indexer)
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(names, ["NZBgeek", "Another"], "ordered by priority");

    repo.delete(id).await.unwrap();
    assert!(matches!(repo.get(id).await, Err(DbError::NotFound)));
}

#[tokio::test]
async fn provider_back_off_is_persisted() {
    let db = Db::in_memory().await.unwrap();
    let repo = db.providers();
    let id = repo.insert(&indexer("Flaky")).await.unwrap();
    let started = t(-1); // long before "now": no startup grace
    let now = t(10);

    assert!(
        repo.blocked(ProviderKind::Indexer, now)
            .await
            .unwrap()
            .is_empty()
    );
    let s = repo
        .record_failure(id, now, started, None, true)
        .await
        .unwrap();
    assert_eq!(s.escalation_level, 1);
    assert_eq!(s.disabled_till, Some(now + Duration::minutes(1)));
    assert_eq!(repo.status(id).await.unwrap(), s, "stored");

    let blocked = repo.blocked(ProviderKind::Indexer, now).await.unwrap();
    assert_eq!(blocked, [(id, now + Duration::minutes(1))]);
    assert!(
        repo.blocked(ProviderKind::DownloadClient, now)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        repo.blocked(ProviderKind::Indexer, now + Duration::minutes(2))
            .await
            .unwrap()
            .is_empty(),
        "window over"
    );

    repo.record_failure(id, now + Duration::minutes(5), started, None, true)
        .await
        .unwrap();
    assert_eq!(repo.status(id).await.unwrap().escalation_level, 2);
    repo.record_success(id).await.unwrap();
    let after = repo.status(id).await.unwrap();
    assert_eq!(after.escalation_level, 1, "one step down");
    assert_eq!(after.disabled_till, None);

    repo.delete(id).await.unwrap();
    let orphans: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_status")
        .fetch_one(db.main_pool())
        .await
        .unwrap();
    assert_eq!(orphans, 0, "status row went with the provider");
}
