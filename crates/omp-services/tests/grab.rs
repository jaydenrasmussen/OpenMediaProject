use std::{sync::Arc, time::Duration as StdDuration};

use chrono::{DateTime, Duration, TimeZone, Utc};
use omp_core::{
    history::HistoryEvent,
    language::Language,
    movie::{MovieMetadata, MovieStatus},
    quality::Quality,
    release::{DownloadProtocol, IndexerFlags, ReleaseInfo},
};
use omp_db::{
    Db,
    repo::{NewMovie, PendingReason, Provider, ProviderKind},
};
use omp_decision::DecisionEngine;
use omp_downloads::{ClientError, ClientRegistry, FakeClient, RegisteredClient};
use omp_quality::profile::{ProfileItem, QualityProfile};
use omp_services::{
    EvaluateRequest, Evaluation, GrabContext, NoDiskCheck, ProcessedDecisions, QueueMap,
    evaluate_releases, process_decisions,
};

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

fn release(title: &str, protocol: DownloadProtocol) -> ReleaseInfo {
    ReleaseInfo {
        guid: format!("guid-{title}"),
        title: title.into(),
        size: 4 * 1024 * 1024 * 1024,
        download_url: format!("https://idx.example/{title}.nzb"),
        indexer_id: 0,
        indexer: "Idx".into(),
        indexer_priority: 25,
        protocol,
        publish_date: now() - Duration::hours(2),
        container: None,
        indexer_flags: IndexerFlags::G_FREELEECH,
        imdb_id: None,
        tmdb_id: 0,
        torrent: None,
    }
}

fn usenet(title: &str) -> ReleaseInfo {
    release(title, DownloadProtocol::Usenet)
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

struct Rig {
    db: Db,
    usenet: Arc<FakeClient>,
    usenet_id: i64,
    registry: ClientRegistry,
}

async fn rig() -> Rig {
    let db = setup().await;
    let usenet = Arc::new(FakeClient::new(DownloadProtocol::Usenet));
    let usenet_id = client_row(&db, "SAB", "sabnzbd").await;
    let registry = ClientRegistry::new(vec![RegisteredClient {
        id: usenet_id,
        name: "SAB".into(),
        priority: 1,
        tags: vec![],
        client: usenet.clone(),
    }]);
    Rig {
        db,
        usenet,
        usenet_id,
        registry,
    }
}

async fn evaluate(db: &Db, releases: Vec<ReleaseInfo>) -> Evaluation {
    evaluate_releases(
        db,
        &DecisionEngine::default(),
        EvaluateRequest {
            releases,
            search: None,
            pushed: false,
            now: now(),
            queue: QueueMap::new(),
            disk: Arc::new(NoDiskCheck),
        },
    )
    .await
    .unwrap()
}

fn ctx() -> GrabContext {
    GrabContext {
        now: now(),
        process_start: now() - Duration::days(1),
        category: Some("movies".into()),
    }
}

async fn grab(r: &Rig, releases: Vec<ReleaseInfo>) -> (Evaluation, ProcessedDecisions) {
    let e = evaluate(&r.db, releases).await;
    let p = process_decisions(&r.db, &e, &r.registry, &ctx())
        .await
        .unwrap();
    (e, p)
}

const WEB: &str = "The.Matrix.1999.1080p.WEB-DL.x264-GRP";
const BLURAY: &str = "The.Matrix.1999.1080p.BluRay.x264-GRP";
const P720: &str = "The.Matrix.1999.720p.WEB-DL.x264-GRP";

#[tokio::test]
async fn only_the_best_release_for_a_movie_is_grabbed() {
    let r = rig().await;
    let id = add_movie(&r.db, 603, "The Matrix", 1999).await;
    let (_, p) = grab(&r, vec![usenet(P720), usenet(BLURAY), usenet(WEB)]).await;

    assert_eq!(r.usenet.added(), [BLURAY], "one grab, the best");
    assert_eq!(p.grabbed.len(), 1);
    assert_eq!(p.grabbed[0].client_id, r.usenet_id);
    assert!(p.pending.is_empty());

    let history = r.db.history().for_movie(id).await.unwrap();
    assert_eq!(history.len(), 1);
    let h = &history[0];
    assert_eq!(h.event, HistoryEvent::Grabbed);
    assert_eq!(h.source_title, BLURAY);
    assert_eq!(h.download_id.as_deref(), Some("fake-1"));
    assert_eq!(h.data["size"], (4u64 * 1024 * 1024 * 1024).to_string());
    assert_eq!(
        h.data["indexer_flags"],
        IndexerFlags::G_FREELEECH.bits().to_string()
    );
    assert_eq!(h.data["download_client"], "SAB");
    assert_eq!(h.quality.quality, Quality::BLURAY_1080P);
}

#[tokio::test]
async fn each_movie_gets_its_own_grab() {
    let r = rig().await;
    add_movie(&r.db, 603, "The Matrix", 1999).await;
    add_movie(&r.db, 438631, "Dune", 2021).await;
    let (_, p) = grab(
        &r,
        vec![
            usenet(WEB),
            usenet("Dune.2021.1080p.WEB-DL.x264-GRP"),
            usenet(BLURAY),
        ],
    )
    .await;
    let mut added = r.usenet.added();
    added.sort();
    assert_eq!(added, ["Dune.2021.1080p.WEB-DL.x264-GRP", BLURAY]);
    assert_eq!(p.grabbed.len(), 2);
}

#[tokio::test]
async fn rejected_releases_are_never_sent() {
    let r = rig().await;
    add_movie(&r.db, 603, "The Matrix", 1999).await;
    let (_, p) = grab(&r, vec![usenet("Unknown.Film.2020.1080p.WEB-DL.x264-GRP")]).await;
    assert!(r.usenet.added().is_empty());
    assert!(p.grabbed.is_empty());
    assert_eq!(p.rejected, [0]);
}

#[tokio::test]
async fn delayed_releases_go_to_pending_once() {
    let r = rig().await;
    let id = add_movie(&r.db, 603, "The Matrix", 1999).await;
    let mut profile = r.db.delay_profiles().list().await.unwrap().remove(0);
    profile.usenet_delay = 360; // 6 h; the releases are 2 h old
    r.db.delay_profiles().update(&profile).await.unwrap();

    let (_, p) = grab(&r, vec![usenet(BLURAY), usenet(WEB)]).await;
    assert!(
        r.usenet.added().is_empty(),
        "nothing sent while the delay runs"
    );
    assert_eq!(p.pending.len(), 2);

    let rows = r.db.pending_releases().for_movie(id).await.unwrap();
    assert_eq!(rows.len(), 2);
    let reasons: Vec<_> = rows.iter().map(|x| x.reason).collect();
    assert!(reasons.contains(&PendingReason::Delay));
    assert!(
        reasons.contains(&PendingReason::Fallback),
        "second release for the movie is a fallback: {reasons:?}"
    );

    // Running the same batch again does not pile up duplicates.
    grab(&r, vec![usenet(BLURAY), usenet(WEB)]).await;
    assert_eq!(
        r.db.pending_releases().for_movie(id).await.unwrap().len(),
        2
    );
}

#[tokio::test]
async fn the_best_in_profile_bypasses_the_delay_and_the_lower_release_is_skipped() {
    let r = rig().await;
    add_movie(&r.db, 603, "The Matrix", 1999).await;
    // The Bluray (best quality in the profile) bypasses the delay; the 720p
    // would have to wait but is skipped because the movie is already grabbed.
    let mut profile = r.db.delay_profiles().list().await.unwrap().remove(0);
    profile.usenet_delay = 360;
    profile.bypass_if_highest_quality = true;
    r.db.delay_profiles().update(&profile).await.unwrap();

    let (_, p) = grab(&r, vec![usenet(P720), usenet(BLURAY)]).await;
    assert_eq!(
        r.usenet.added(),
        [BLURAY],
        "the best-in-profile release skipped the delay"
    );
    assert_eq!(p.grabbed.len(), 1);
    assert!(
        p.pending.is_empty(),
        "the 720p was skipped because the movie was grabbed"
    );
}

#[tokio::test]
async fn an_unavailable_client_parks_releases_and_is_not_retried_in_the_batch() {
    let r = rig().await;
    let a = add_movie(&r.db, 603, "The Matrix", 1999).await;
    let b = add_movie(&r.db, 438631, "Dune", 2021).await;
    r.usenet
        .fail_with(Some(ClientError::Unavailable("connection refused".into())));

    let (_, p) = grab(
        &r,
        vec![
            usenet(BLURAY),
            usenet(WEB),
            usenet("Dune.2021.1080p.WEB-DL.x264-GRP"),
        ],
    )
    .await;
    assert!(r.usenet.added().is_empty());
    assert!(p.grabbed.is_empty());
    assert_eq!(p.pending.len(), 3);

    let first = r.db.pending_releases().for_movie(a).await.unwrap();
    let reasons: Vec<_> = first.iter().map(|x| x.reason).collect();
    assert!(reasons.contains(&PendingReason::DownloadClientUnavailable));
    assert!(reasons.contains(&PendingReason::Fallback), "{reasons:?}");
    assert_eq!(
        r.db.pending_releases().for_movie(b).await.unwrap()[0].reason,
        PendingReason::DownloadClientUnavailable,
        "the other movie was parked without trying again"
    );

    // The client is now backed off.
    let status = r.db.providers().status(r.usenet_id).await.unwrap();
    assert_eq!(status.escalation_level, 1);
    assert!(status.is_disabled(now()));
}

#[tokio::test]
async fn no_client_for_the_protocol_parks_the_release() {
    let r = rig().await;
    let id = add_movie(&r.db, 603, "The Matrix", 1999).await;
    let (_, p) = grab(&r, vec![release(WEB, DownloadProtocol::Torrent)]).await;
    assert_eq!(p.pending.len(), 1);
    assert_eq!(
        r.db.pending_releases().for_movie(id).await.unwrap()[0].reason,
        PendingReason::DownloadClientUnavailable
    );
}

#[tokio::test]
async fn a_release_that_is_gone_is_rejected_not_parked() {
    let r = rig().await;
    let id = add_movie(&r.db, 603, "The Matrix", 1999).await;
    r.usenet
        .fail_with(Some(ClientError::ReleaseUnavailable("expired".into())));
    let (_, p) = grab(&r, vec![usenet(WEB)]).await;
    assert_eq!(p.rejected, [0]);
    assert!(p.pending.is_empty() && p.grabbed.is_empty());
    assert!(
        r.db.pending_releases()
            .for_movie(id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(r.db.history().for_movie(id).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_failed_release_fetch_backs_off_the_indexer_by_the_requested_time() {
    let r = rig().await;
    add_movie(&r.db, 603, "The Matrix", 1999).await;
    let indexer =
        r.db.providers()
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
    r.usenet.fail_with(Some(ClientError::ReleaseDownloadFailed {
        message: "429".into(),
        retry_after: Some(StdDuration::from_secs(2 * 3600)),
    }));
    let mut rel = usenet(WEB);
    rel.indexer_id = indexer as i32;
    let (_, p) = grab(&r, vec![rel]).await;
    assert!(
        p.grabbed.is_empty() && p.pending.is_empty() && p.rejected.is_empty(),
        "skipped"
    );

    let status = r.db.providers().status(indexer).await.unwrap();
    // A two hour minimum raises the level until the window is long enough.
    assert_eq!(
        status.disabled_till,
        Some(now() + Duration::hours(3)),
        "{status:?}"
    );
}

#[tokio::test]
async fn a_client_that_rejects_a_release_is_skipped_without_penalty() {
    let r = rig().await;
    add_movie(&r.db, 603, "The Matrix", 1999).await;
    r.usenet
        .fail_with(Some(ClientError::Rejected("duplicate".into())));
    let (_, p) = grab(&r, vec![usenet(WEB)]).await;
    assert!(p.grabbed.is_empty() && p.pending.is_empty());
    assert_eq!(
        r.db.providers()
            .status(r.usenet_id)
            .await
            .unwrap()
            .escalation_level,
        0
    );
}

#[tokio::test]
async fn a_successful_grab_steps_the_client_back_up() {
    let r = rig().await;
    add_movie(&r.db, 603, "The Matrix", 1999).await;
    // One earlier failure, long ago.
    r.db.providers()
        .record_failure(
            r.usenet_id,
            now() - Duration::days(1),
            now() - Duration::days(2),
            None,
            true,
        )
        .await
        .unwrap();
    assert_eq!(
        r.db.providers()
            .status(r.usenet_id)
            .await
            .unwrap()
            .escalation_level,
        1
    );
    grab(&r, vec![usenet(WEB)]).await;
    assert_eq!(
        r.db.providers()
            .status(r.usenet_id)
            .await
            .unwrap()
            .escalation_level,
        0
    );
}

#[tokio::test]
async fn a_delayed_best_release_does_not_stop_a_lower_one_that_can_go_now() {
    let r = rig().await;
    let id = add_movie(&r.db, 603, "The Matrix", 1999).await;
    let torrent = Arc::new(FakeClient::new(DownloadProtocol::Torrent));
    let torrent_id = client_row(&r.db, "Transmission", "transmission").await;
    let registry = ClientRegistry::new(vec![
        RegisteredClient {
            id: r.usenet_id,
            name: "SAB".into(),
            priority: 1,
            tags: vec![],
            client: r.usenet.clone(),
        },
        RegisteredClient {
            id: torrent_id,
            name: "Transmission".into(),
            priority: 1,
            tags: vec![],
            client: torrent.clone(),
        },
    ]);
    // Usenet waits 6 hours, torrents do not wait at all.
    let mut profile = r.db.delay_profiles().list().await.unwrap().remove(0);
    profile.usenet_delay = 360;
    profile.torrent_delay = 0;
    r.db.delay_profiles().update(&profile).await.unwrap();

    let releases = vec![usenet(BLURAY), release(P720, DownloadProtocol::Torrent)];
    let e = evaluate(&r.db, releases).await;
    let p = process_decisions(&r.db, &e, &registry, &ctx())
        .await
        .unwrap();

    assert_eq!(
        torrent.added(),
        [P720],
        "the lower-quality torrent was grabbed now"
    );
    assert!(r.usenet.added().is_empty());
    assert_eq!(p.grabbed.len(), 1);
    let pending = r.db.pending_releases().for_movie(id).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].release.title, BLURAY);
    assert_eq!(
        pending[0].reason,
        PendingReason::Delay,
        "the better release still waits for its delay"
    );
}

#[tokio::test]
async fn a_chosen_release_is_grabbed_even_if_the_engine_rejected_it() {
    use omp_services::grab_selected;
    let r = rig().await;
    let id = add_movie(&r.db, 603, "The Matrix", 1999).await;
    // An unmonitored movie is a permanent rejection for automatic grabs.
    let m = r.db.movies().get(id).await.unwrap();
    r.db.movies()
        .update(
            id,
            &omp_db::repo::MovieUpdate {
                monitored: false,
                minimum_availability: m.movie.minimum_availability,
                quality_profile_id: m.movie.quality_profile_id,
                path: m.movie.path.clone(),
                root_folder_path: None,
                tags: vec![],
            },
        )
        .await
        .unwrap();
    let e = evaluate(&r.db, vec![usenet("The.Matrix.1999.1080p.BluRay.x264-GRP")]).await;
    assert!(e.items[0].decision.rejected(), "{:?}", e.items[0].decision);

    let grabbed = grab_selected(&r.db, &e, &r.registry, &ctx(), 0)
        .await
        .unwrap();
    assert_eq!(r.usenet.added(), ["The.Matrix.1999.1080p.BluRay.x264-GRP"]);
    assert_eq!(grabbed.client_id, r.usenet_id);
    let h = r.db.history().for_movie(id).await.unwrap();
    assert_eq!((h.len(), h[0].event), (1, HistoryEvent::Grabbed));
    assert_eq!(
        h[0].download_id.as_deref(),
        Some(grabbed.download_id.as_str())
    );
}

#[tokio::test]
async fn choosing_a_release_reports_why_it_could_not_be_sent() {
    use omp_services::grab_selected;
    let r = rig().await;
    add_movie(&r.db, 603, "The Matrix", 1999).await;

    let unmatched = evaluate(
        &r.db,
        vec![usenet("Some.Other.Film.2020.1080p.WEB-DL.x264-GRP")],
    )
    .await;
    let err = grab_selected(&r.db, &unmatched, &r.registry, &ctx(), 0)
        .await
        .unwrap_err();
    assert!(err.contains("could not be matched"), "{err}");

    let e = evaluate(&r.db, vec![usenet(WEB)]).await;
    r.usenet
        .fail_with(Some(ClientError::Unavailable("connection refused".into())));
    let err = grab_selected(&r.db, &e, &r.registry, &ctx(), 0)
        .await
        .unwrap_err();
    assert!(err.contains("connection refused"), "{err}");
    assert_eq!(
        r.db.providers()
            .status(r.usenet_id)
            .await
            .unwrap()
            .escalation_level,
        1,
        "the client is backed off"
    );

    let torrent = evaluate(&r.db, vec![release(WEB, DownloadProtocol::Torrent)]).await;
    r.usenet.fail_with(None);
    let err = grab_selected(&r.db, &torrent, &r.registry, &ctx(), 0)
        .await
        .unwrap_err();
    assert!(err.contains("isn't configured"), "no torrent client: {err}");
}
