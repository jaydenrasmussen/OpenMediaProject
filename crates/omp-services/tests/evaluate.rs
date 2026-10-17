use std::{collections::BTreeMap, sync::Arc};

use chrono::{DateTime, Duration, TimeZone, Utc};
use omp_core::{
    blocklist::BlocklistEntry,
    history::HistoryEvent,
    language::Language,
    movie::{MovieMetadata, MovieStatus},
    quality::{Quality, QualityModel},
    release::{DownloadProtocol, IndexerFlags, ReleaseInfo},
};
use omp_db::{
    Db,
    repo::{HistoryRecord, MovieFileRecord, MovieRecord, NewMovie, Provider, ProviderKind},
};
use omp_decision::{DecisionEngine, RejectionKind, RejectionReason as R, SearchContext};
use omp_quality::{
    custom_format::{ConditionDef, CustomFormatDef, SpecificationDef},
    profile::{ProfileFormatItem, ProfileItem, QualityProfile},
    release_profile::ReleaseProfile,
};
use omp_services::{
    DiskSpace, EvaluateRequest, Evaluation, NoDiskCheck, QueueMap, evaluate_releases,
};

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap()
}

struct World {
    db: Db,
    profile: i32,
    movie: MovieRecord,
}

fn profile(language: Language) -> QualityProfile {
    let q = |quality, allowed| ProfileItem::Quality { quality, allowed };
    QualityProfile {
        id: 0,
        name: "HD".into(),
        cutoff: Quality::WEBDL_1080P.id,
        items: vec![
            q(Quality::SDTV, false),
            q(Quality::DVD, true),
            q(Quality::WEBDL_720P, true),
            q(Quality::WEBDL_1080P, true),
            q(Quality::BLURAY_1080P, true),
        ],
        min_format_score: 0,
        cutoff_format_score: 0,
        min_upgrade_format_score: 1,
        format_items: vec![],
        language,
        upgrade_allowed: true,
    }
}

async fn world() -> World {
    let db = Db::in_memory().await.unwrap();
    db.delay_profiles().seed_default().await.unwrap();
    let profile_id = db
        .quality_profiles()
        .insert(&profile(Language::ANY))
        .await
        .unwrap();
    let metadata = MovieMetadata {
        tmdb_id: 603,
        imdb_id: Some("tt0133093".into()),
        title: "The Matrix".into(),
        original_title: "The Matrix".into(),
        original_language: "en".into(),
        year: 1999,
        runtime: 136,
        status: MovieStatus::Released,
        in_cinemas: Some(now() - Duration::days(9000)),
        ..Default::default()
    };
    let metadata_id = db.movies().upsert_metadata(&metadata).await.unwrap();
    let id = db
        .movies()
        .add(&NewMovie {
            metadata_id,
            monitored: true,
            minimum_availability: MovieStatus::Announced,
            quality_profile_id: profile_id,
            path: "/movies/The Matrix (1999)".into(),
            root_folder_path: None,
            tags: vec![],
        })
        .await
        .unwrap();
    let movie = db.movies().get(id).await.unwrap();
    World {
        db,
        profile: profile_id,
        movie,
    }
}

fn release(title: &str) -> ReleaseInfo {
    ReleaseInfo {
        guid: format!("guid-{title}"),
        title: title.into(),
        size: 4 * 1024 * 1024 * 1024,
        download_url: format!("https://idx.example/{title}.nzb"),
        indexer_id: 1,
        indexer: "Idx".into(),
        indexer_priority: 25,
        protocol: DownloadProtocol::Usenet,
        publish_date: now() - Duration::hours(2),
        container: None,
        indexer_flags: IndexerFlags::NONE,
        imdb_id: None,
        tmdb_id: 0,
        torrent: None,
    }
}

async fn run(w: &World, releases: Vec<ReleaseInfo>, search: Option<SearchContext>) -> Evaluation {
    run_with(w, releases, search, Arc::new(NoDiskCheck)).await
}

async fn run_with(
    w: &World,
    releases: Vec<ReleaseInfo>,
    search: Option<SearchContext>,
    disk: Arc<dyn DiskSpace>,
) -> Evaluation {
    evaluate_releases(
        &w.db,
        &DecisionEngine::default(),
        EvaluateRequest {
            releases,
            search,
            pushed: false,
            now: now(),
            queue: QueueMap::new(),
            disk,
        },
    )
    .await
    .unwrap()
}

fn reasons(e: &Evaluation, i: usize) -> Vec<R> {
    e.items[i]
        .decision
        .rejections
        .iter()
        .map(|r| r.reason)
        .collect()
}

const WEB: &str = "The.Matrix.1999.1080p.WEB-DL.x264-GRP";

#[tokio::test]
async fn a_good_release_for_a_wanted_movie_is_approved() {
    let w = world().await;
    let e = run(&w, vec![release(WEB)], None).await;
    assert_eq!(e.items.len(), 1);
    let item = &e.items[0];
    assert!(item.decision.approved(), "{:?}", item.decision.rejections);
    assert_eq!(item.movie.as_ref().unwrap().movie.tmdb_id, 603);
    assert_eq!(
        item.parsed.as_ref().unwrap().quality.quality,
        Quality::WEBDL_1080P
    );
}

#[tokio::test]
async fn unknown_and_unparsable_releases() {
    let w = world().await;
    let releases = vec![
        release("Some.Other.Film.2020.1080p.WEB-DL.x264-GRP"),
        release("!!!"),
    ];

    // RSS drops what it cannot parse but still reports unknown movies.
    let rss = run(&w, releases.clone(), None).await;
    assert_eq!(rss.items.len(), 1);
    assert_eq!(reasons(&rss, 0), [R::UnknownMovie]);

    // A search reports both.
    let search = SearchContext {
        movie_id: w.movie.movie.id,
        user_invoked: false,
        interactive: false,
    };
    let searched = run(&w, releases, Some(search)).await;
    assert_eq!(searched.items.len(), 2);
    assert_eq!(reasons(&searched, 1), [R::UnableToParse]);
}

#[tokio::test]
async fn the_profile_decides_which_qualities_are_wanted() {
    let w = world().await;
    let e = run(
        &w,
        vec![
            release("The.Matrix.1999.SDTV.x264-GRP"),
            release("The.Matrix.1999.720p.WEB-DL.x264-GRP"),
        ],
        None,
    )
    .await;
    assert!(reasons(&e, 0).contains(&R::QualityNotWanted));
    assert!(e.items[1].decision.approved());
}

#[tokio::test]
async fn blocklisted_releases_are_rejected() {
    let w = world().await;
    let r = release(WEB);
    w.db.blocklist()
        .insert(&BlocklistEntry {
            id: 0,
            movie_id: w.movie.movie.id,
            source_title: r.title.clone(),
            quality: QualityModel::default(),
            languages: vec![],
            published_date: Some(r.publish_date),
            size: Some(r.size),
            protocol: DownloadProtocol::Usenet,
            indexer: Some("Idx".into()),
            indexer_flags: IndexerFlags::NONE,
            torrent_info_hash: None,
            message: None,
            date: now(),
        })
        .await
        .unwrap();
    let e = run(&w, vec![r], None).await;
    assert_eq!(reasons(&e, 0), [R::Blocklisted]);
}

async fn give_file(w: &World, quality: Quality) {
    w.db.movie_files()
        .set(&MovieFileRecord {
            id: 0,
            movie_id: w.movie.movie.id,
            relative_path: "The Matrix (1999).mkv".into(),
            size: 5_000_000_000,
            date_added: now() - Duration::days(30),
            scene_name: None,
            release_group: Some("OLD".into()),
            edition: None,
            quality: QualityModel {
                quality,
                ..Default::default()
            },
            languages: vec![Language::ENGLISH],
            media_info: None,
            original_file_path: None,
            indexer_flags: IndexerFlags::NONE,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn an_existing_file_at_the_cutoff_ends_the_search_for_upgrades() {
    let w = world().await;
    give_file(&w, Quality::WEBDL_1080P).await;
    let e = run(
        &w,
        vec![release("The.Matrix.1999.1080p.BluRay.x264-GRP")],
        None,
    )
    .await;
    assert!(
        reasons(&e, 0).contains(&R::DiskCutoffMet),
        "{:?}",
        reasons(&e, 0)
    );
}

#[tokio::test]
async fn a_file_below_the_cutoff_can_be_upgraded() {
    let w = world().await;
    give_file(&w, Quality::DVD).await;
    let e = run(&w, vec![release(WEB)], None).await;
    assert!(e.items[0].decision.approved(), "{:?}", reasons(&e, 0));
    assert!(e.items[0].existing_file.is_some());
}

fn grab(w: &World, quality: Quality, age: Duration) -> HistoryRecord {
    HistoryRecord {
        id: 0,
        movie_id: w.movie.movie.id,
        event: HistoryEvent::Grabbed,
        date: now() - age,
        source_title: "The.Matrix.1999.1080p.WEB-DL.x264-GRP".into(),
        quality: QualityModel {
            quality,
            ..Default::default()
        },
        languages: vec![Language::ENGLISH],
        download_id: Some("ABC".into()),
        data: BTreeMap::new(),
    }
}

#[tokio::test]
async fn a_recent_grab_blocks_rss_but_not_a_search() {
    let w = world().await;
    w.db.history()
        .insert(&grab(&w, Quality::WEBDL_1080P, Duration::hours(1)))
        .await
        .unwrap();

    let rss = run(
        &w,
        vec![release("The.Matrix.1999.1080p.BluRay.x264-GRP")],
        None,
    )
    .await;
    assert!(
        reasons(&rss, 0).contains(&R::HistoryRecentCutoffMet),
        "{:?}",
        reasons(&rss, 0)
    );

    let search = SearchContext {
        movie_id: w.movie.movie.id,
        user_invoked: true,
        interactive: true,
    };
    let searched = run(
        &w,
        vec![release("The.Matrix.1999.1080p.BluRay.x264-GRP")],
        Some(search),
    )
    .await;
    assert!(!reasons(&searched, 0).contains(&R::HistoryRecentCutoffMet));
}

#[tokio::test]
async fn custom_format_scores_come_from_the_profile() {
    let w = world().await;
    let fmt =
        w.db.custom_formats()
            .insert(&CustomFormatDef {
                id: 0,
                name: "x264".into(),
                include_when_renaming: false,
                specifications: vec![SpecificationDef {
                    name: "title".into(),
                    negate: false,
                    required: false,
                    condition: ConditionDef::ReleaseTitle {
                        value: "x264".into(),
                    },
                }],
            })
            .await
            .unwrap();
    let mut p = w.db.quality_profiles().get(w.profile).await.unwrap();
    p.format_items = vec![ProfileFormatItem {
        format_id: fmt,
        score: -50,
    }];
    p.min_format_score = -10;
    w.db.quality_profiles().update(&p).await.unwrap();

    let e = run(
        &w,
        vec![
            release(WEB),
            release("The.Matrix.1999.1080p.WEB-DL.x265-GRP"),
        ],
        None,
    )
    .await;
    assert_eq!(e.items[0].custom_format_score, -50);
    assert_eq!(e.items[0].custom_format_ids, [fmt]);
    assert!(reasons(&e, 0).contains(&R::CustomFormatMinimumScore));
    assert_eq!(e.items[1].custom_format_score, 0);
    assert!(e.items[1].decision.approved());
}

#[tokio::test]
async fn a_blocked_indexer_is_a_temporary_rejection() {
    let w = world().await;
    let id =
        w.db.providers()
            .insert(&Provider {
                id: 0,
                kind: ProviderKind::Indexer,
                name: "Idx".into(),
                implementation: "newznab".into(),
                plugin_id: None,
                settings: serde_json::json!({}),
                tags: vec![],
                enabled: true,
                enable_rss: true,
                enable_automatic_search: true,
                enable_interactive_search: true,
                priority: 25,
            })
            .await
            .unwrap();
    // The first failure blocks for a minute, starting now.
    w.db.providers()
        .record_failure(id, now(), now() - Duration::days(1), None, true)
        .await
        .unwrap();

    let mut r = release(WEB);
    r.indexer_id = id as i32;
    let e = run(&w, vec![r], None).await;
    assert_eq!(reasons(&e, 0), [R::IndexerDisabled]);
    assert!(e.items[0].decision.temporarily_rejected());
    assert_eq!(
        e.items[0].decision.rejections[0].kind,
        RejectionKind::Temporary
    );
}

#[tokio::test]
async fn delay_profiles_hold_releases_back() {
    let w = world().await;
    let mut default = w.db.delay_profiles().list().await.unwrap().remove(0);
    default.usenet_delay = 360; // 6 hours; the release is 2 hours old
    w.db.delay_profiles().update(&default).await.unwrap();
    let e = run(&w, vec![release(WEB)], None).await;
    assert_eq!(reasons(&e, 0), [R::MinimumAgeDelay]);
    assert!(e.items[0].decision.temporarily_rejected());

    // A user search ignores the delay.
    let search = SearchContext {
        movie_id: w.movie.movie.id,
        user_invoked: true,
        interactive: false,
    };
    let searched = run(&w, vec![release(WEB)], Some(search)).await;
    assert!(searched.items[0].decision.approved());
}

#[tokio::test]
async fn release_profiles_filter_by_title_terms() {
    let w = world().await;
    w.db.release_profiles()
        .insert(&ReleaseProfile {
            id: 0,
            name: "no x264".into(),
            enabled: true,
            required: vec![],
            ignored: vec!["x264".into()],
            indexer_id: 0,
            tags: vec![],
        })
        .await
        .unwrap();
    let e = run(&w, vec![release(WEB)], None).await;
    assert_eq!(reasons(&e, 0), [R::MustNotContainPresent]);
}

struct TinyDisk;
impl DiskSpace for TinyDisk {
    fn free_bytes(&self, _: &str) -> Option<u64> {
        Some(1024 * 1024)
    }
}

#[tokio::test]
async fn free_space_is_checked_last() {
    let w = world().await;
    let e = run_with(&w, vec![release(WEB)], None, Arc::new(TinyDisk)).await;
    assert_eq!(reasons(&e, 0), [R::MinimumFreeSpace]);

    // If something earlier already rejects, the disk is not consulted.
    let mut unmonitored = w.db.movies().get(w.movie.movie.id).await.unwrap();
    unmonitored.movie.monitored = false;
    w.db.movies()
        .update(
            w.movie.movie.id,
            &omp_db::repo::MovieUpdate {
                monitored: false,
                minimum_availability: MovieStatus::Announced,
                quality_profile_id: w.profile,
                path: unmonitored.movie.path.clone(),
                root_folder_path: None,
                tags: vec![],
            },
        )
        .await
        .unwrap();
    let e = run_with(&w, vec![release(WEB)], None, Arc::new(TinyDisk)).await;
    assert_eq!(reasons(&e, 0), [R::MovieNotMonitored]);
}

#[tokio::test]
async fn approved_releases_are_ranked_per_movie_best_first() {
    let w = world().await;
    let e = run(
        &w,
        vec![
            release("The.Matrix.1999.720p.WEB-DL.x264-GRP"),
            release("The.Matrix.1999.1080p.BluRay.x264-GRP"),
            release(WEB),
            release("Some.Other.Film.2020.1080p.WEB-DL.x264-GRP"),
        ],
        None,
    )
    .await;
    let groups = e.approved_best_first();
    assert_eq!(
        groups.len(),
        1,
        "one movie, the unknown film is not approved"
    );
    let titles: Vec<_> = groups[0]
        .iter()
        .map(|&i| e.items[i].release.title.as_str())
        .collect();
    assert_eq!(
        titles,
        [
            "The.Matrix.1999.1080p.BluRay.x264-GRP",
            "The.Matrix.1999.1080p.WEB-DL.x264-GRP",
            "The.Matrix.1999.720p.WEB-DL.x264-GRP"
        ]
    );
}

#[tokio::test]
async fn the_decision_settings_come_from_the_config_table() {
    let w = world().await;
    let mut config = omp_services::load_decision_config(&w.db).await.unwrap();
    assert_eq!(config.maximum_size, 0);
    config.maximum_size = 1000; // MB; the release is 4 GB
    omp_services::save_decision_config(&w.db, &config)
        .await
        .unwrap();
    let e = run(&w, vec![release(WEB)], None).await;
    assert_eq!(reasons(&e, 0), [R::MaximumSizeExceeded]);
}

#[tokio::test]
async fn delay_profile_default_is_used_when_none_is_stored() {
    // A fresh database without the seeded profile still evaluates (the snapshot
    // falls back to the catch-all profile).
    let w = world().await;
    sqlx_clear_delay_profiles(&w.db).await;
    assert!(w.db.delay_profiles().list().await.unwrap().is_empty());
    let e = run(&w, vec![release(WEB)], None).await;
    assert!(e.items[0].decision.approved(), "{:?}", reasons(&e, 0));
}

async fn sqlx_clear_delay_profiles(db: &Db) {
    sqlx::query("DELETE FROM delay_profiles")
        .execute(db.main_pool())
        .await
        .unwrap();
}
