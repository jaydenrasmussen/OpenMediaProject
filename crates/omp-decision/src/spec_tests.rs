//! One or more tests per specification, plus the engine and ranking.

use chrono::Duration;
use omp_core::{
    language::Language as Lang,
    movie::MovieStatus,
    quality::{Quality, QualityModel, Revision},
    release::{DownloadProtocol, IndexerFlags, TorrentRelease},
};
use omp_quality::{
    definition::{QualityDefinition, QualityDefinitions},
    release_profile::ReleaseProfile,
    upgrade::ProperDownloadTypes,
};

use crate::{
    DecisionEngine, DecisionInfo,
    model::*,
    reason::{RejectionKind, RejectionReason as R, Verdict},
    specs::*,
    testkit::*,
};

fn verdict(spec: &dyn Specification, fx: &Fx, env: &Env) -> Verdict {
    spec.evaluate(&fx.subject(), &DecisionInfo::default(), env)
}

fn verdict_with(spec: &dyn Specification, fx: &Fx, env: &Env, info: DecisionInfo) -> Verdict {
    spec.evaluate(&fx.subject(), &info, env)
}

fn reason(v: Verdict) -> Option<R> {
    match v {
        Verdict::Accept => None,
        Verdict::Reject { reason, .. } => Some(reason),
    }
}

fn user_search(movie_id: i64) -> DecisionInfo {
    DecisionInfo {
        search: Some(SearchContext {
            movie_id,
            user_invoked: true,
            interactive: false,
        }),
        ..Default::default()
    }
}

fn scheduled_search(movie_id: i64) -> DecisionInfo {
    DecisionInfo {
        search: Some(SearchContext {
            movie_id,
            user_invoked: false,
            interactive: false,
        }),
        ..Default::default()
    }
}

// ---- baseline ----------------------------------------------------------------

#[test]
fn a_plain_release_for_a_wanted_movie_is_approved_by_every_spec() {
    let fx = Fx::new();
    let env = Env::new();
    let decision = DecisionEngine::default().decide(&fx.subject(), &DecisionInfo::default(), &env);
    assert!(decision.approved(), "{:?}", decision.rejections);
}

// ---- size --------------------------------------------------------------------

fn defs_with(quality: Quality, min: f64, max: Option<f64>) -> QualityDefinitions {
    QualityDefinitions::new(vec![QualityDefinition {
        quality,
        weight: 1,
        min_size: Some(min),
        max_size: max,
        preferred_size: Some(95.0),
    }])
}

#[test]
fn acceptable_size_checks_megabytes_per_minute_of_runtime() {
    let fx = Fx::new(); // 4 GB, 120 minutes => ~34 MB/min
    let mut env = Env::new();
    env.definitions = defs_with(Quality::WEBDL_1080P, 10.0, Some(100.0));
    assert!(verdict(&AcceptableSize, &fx, &env).is_accept());

    env.definitions = defs_with(Quality::WEBDL_1080P, 50.0, Some(100.0));
    assert_eq!(
        reason(verdict(&AcceptableSize, &fx, &env)),
        Some(R::BelowMinimumSize)
    );

    env.definitions = defs_with(Quality::WEBDL_1080P, 0.0, Some(20.0));
    assert_eq!(
        reason(verdict(&AcceptableSize, &fx, &env)),
        Some(R::AboveMaximumSize)
    );

    env.definitions = defs_with(Quality::WEBDL_1080P, 0.0, None);
    assert!(
        verdict(&AcceptableSize, &fx, &env).is_accept(),
        "no maximum"
    );
    env.definitions = defs_with(Quality::WEBDL_1080P, 0.0, Some(0.0));
    assert!(
        verdict(&AcceptableSize, &fx, &env).is_accept(),
        "0 means unlimited"
    );
}

#[test]
fn acceptable_size_skips_unknown_sizes_and_assumes_110_minutes_without_runtime() {
    let mut fx = Fx::new();
    let mut env = Env::new();
    env.definitions = defs_with(Quality::WEBDL_1080P, 0.0, Some(20.0));
    fx.release.size = 0;
    assert!(verdict(&AcceptableSize, &fx, &env).is_accept());

    fx.release.size = 20 * 1024 * 1024 * 110 + 1; // just over 20 MB/min for 110 min
    fx.metadata.runtime = 0;
    assert_eq!(
        reason(verdict(&AcceptableSize, &fx, &env)),
        Some(R::AboveMaximumSize)
    );
    fx.release.size -= 1;
    assert!(verdict(&AcceptableSize, &fx, &env).is_accept());
}

#[test]
fn maximum_size_setting() {
    let fx = Fx::new();
    let mut env = Env::new();
    assert!(verdict(&MaximumSize, &fx, &env).is_accept(), "0 disables");
    env.config.maximum_size = 3000; // MB
    assert_eq!(
        reason(verdict(&MaximumSize, &fx, &env)),
        Some(R::MaximumSizeExceeded)
    );
    env.config.maximum_size = 5000;
    assert!(verdict(&MaximumSize, &fx, &env).is_accept());
}

#[test]
fn sample_detection() {
    let mut fx = Fx::new();
    let env = Env::new();
    fx.release.title = "Movie.2020.SAMPLE.mkv".into();
    fx.release.size = 10 * 1024 * 1024;
    assert_eq!(reason(verdict(&NotSample, &fx, &env)), Some(R::Sample));
    fx.release.size = 100 * 1024 * 1024;
    assert!(
        verdict(&NotSample, &fx, &env).is_accept(),
        "big enough to be real"
    );
}

// ---- usenet age and retention --------------------------------------------------

#[test]
fn minimum_age_is_usenet_only_and_temporary() {
    let mut fx = Fx::new(); // 60 minutes old
    let mut env = Env::new();
    env.config.minimum_age = 120;
    assert_eq!(reason(verdict(&MinimumAge, &fx, &env)), Some(R::MinimumAge));
    assert_eq!(MinimumAge.kind(), RejectionKind::Temporary);
    env.config.minimum_age = 30;
    assert!(verdict(&MinimumAge, &fx, &env).is_accept());
    env.config.minimum_age = 120;
    fx.release.protocol = DownloadProtocol::Torrent;
    assert!(verdict(&MinimumAge, &fx, &env).is_accept());
}

#[test]
fn retention_is_usenet_only() {
    let mut fx = Fx::new();
    let mut env = Env::new();
    fx.release.publish_date = now() - Duration::days(30);
    env.config.retention = 10;
    assert_eq!(reason(verdict(&Retention, &fx, &env)), Some(R::MaximumAge));
    env.config.retention = 0;
    assert!(verdict(&Retention, &fx, &env).is_accept());
    env.config.retention = 10;
    fx.release.protocol = DownloadProtocol::Torrent;
    assert!(verdict(&Retention, &fx, &env).is_accept());
}

// ---- protocol, raw discs, language, subs ---------------------------------------

#[test]
fn disabled_protocols_are_rejected() {
    let mut fx = Fx::new();
    let mut env = Env::new();
    env.delay_profiles[0].enable_usenet = false;
    assert_eq!(
        reason(verdict(&Protocol, &fx, &env)),
        Some(R::ProtocolDisabled)
    );
    fx.release.protocol = DownloadProtocol::Torrent;
    assert!(verdict(&Protocol, &fx, &env).is_accept());
    env.delay_profiles[0].enable_torrent = false;
    assert_eq!(
        reason(verdict(&Protocol, &fx, &env)),
        Some(R::ProtocolDisabled)
    );
}

#[test]
fn raw_discs_are_rejected() {
    let mut fx = Fx::new();
    let env = Env::new();
    for title in [
        "Movie.2020.1080p.Full.Bluray",
        "Movie.2020.Disc.1.1080p.Bluray",
        "Movie.2020.DVDR",
        "Movie.2020.DVD-R.GRP",
    ] {
        fx.release.title = title.into();
        assert_eq!(
            reason(verdict(&RawDisk, &fx, &env)),
            Some(R::Raw),
            "{title}"
        );
    }
    fx.release.title = "Movie.2020.1080p.BluRay.x264-GRP".into();
    assert!(verdict(&RawDisk, &fx, &env).is_accept());
    fx.release.container = Some("ISO".into());
    assert_eq!(reason(verdict(&RawDisk, &fx, &env)), Some(R::Raw));
    fx.release.container = Some("m2ts".into());
    assert_eq!(reason(verdict(&RawDisk, &fx, &env)), Some(R::Raw));
    fx.release.container = Some("mkv".into());
    assert!(verdict(&RawDisk, &fx, &env).is_accept());
}

#[test]
fn language_wanted_by_profile() {
    let mut fx = Fx::new();
    let env = Env::new();
    assert!(
        verdict(&Language, &fx, &env).is_accept(),
        "profile wants any"
    );

    fx.profile.language = Lang::GERMAN;
    assert_eq!(
        reason(verdict(&Language, &fx, &env)),
        Some(R::WantedLanguage)
    );
    fx.languages = vec![Lang::ENGLISH, Lang::GERMAN];
    assert!(verdict(&Language, &fx, &env).is_accept());

    fx.profile.language = Lang::ORIGINAL;
    fx.languages = vec![Lang::ENGLISH];
    assert!(
        verdict(&Language, &fx, &env).is_accept(),
        "original language is en"
    );
    fx.metadata.original_language = "fr".into();
    assert_eq!(
        reason(verdict(&Language, &fx, &env)),
        Some(R::WantedLanguage)
    );
}

#[test]
fn hardcoded_subs() {
    let mut fx = Fx::new();
    let mut env = Env::new();
    assert!(
        verdict(&HardcodeSubs, &fx, &env).is_accept(),
        "none present"
    );
    fx.parsed.hardcoded_subs = Some("KORSUBS".into());
    assert_eq!(
        reason(verdict(&HardcodeSubs, &fx, &env)),
        Some(R::HardcodeSubtitles)
    );
    env.config.whitelisted_hardcoded_subs = "kor,fre".into();
    assert!(verdict(&HardcodeSubs, &fx, &env).is_accept(), "whitelisted");
    env.config.whitelisted_hardcoded_subs = ",".into();
    assert_eq!(
        reason(verdict(&HardcodeSubs, &fx, &env)),
        Some(R::HardcodeSubtitles),
        "blank entries match nothing"
    );
    env.config.allow_hardcoded_subs = true;
    assert!(verdict(&HardcodeSubs, &fx, &env).is_accept());
}

// ---- release profiles, indexers -------------------------------------------------

fn profile_with(required: &[&str], ignored: &[&str]) -> ReleaseProfile {
    ReleaseProfile {
        id: 1,
        name: "p".into(),
        enabled: true,
        required: required.iter().map(|s| s.to_string()).collect(),
        ignored: ignored.iter().map(|s| s.to_string()).collect(),
        indexer_id: 0,
        tags: vec![],
        air_date_restriction: false,
        air_date_grace_period_hours: 0,
    }
}

#[test]
fn release_restrictions() {
    let fx = Fx::new(); // "Some.Movie.2020.1080p.WEB-DL.x264-GRP"
    let mut env = Env::new();
    env.release_profiles = vec![profile_with(&["remux", "bluray"], &[])];
    assert_eq!(
        reason(verdict(&ReleaseRestrictions, &fx, &env)),
        Some(R::MustContainMissing)
    );
    env.release_profiles = vec![profile_with(&["web-dl"], &[])];
    assert!(verdict(&ReleaseRestrictions, &fx, &env).is_accept());
    env.release_profiles = vec![profile_with(&[], &["x264", "hdr"])];
    assert_eq!(
        reason(verdict(&ReleaseRestrictions, &fx, &env)),
        Some(R::MustNotContainPresent)
    );
    env.release_profiles = vec![profile_with(&[], &[r"/x26[5]/i"])];
    assert!(
        verdict(&ReleaseRestrictions, &fx, &env).is_accept(),
        "regex term does not match x264"
    );
    env.release_profiles = vec![profile_with(&["/(broken/"], &[])];
    assert_eq!(
        reason(verdict(&ReleaseRestrictions, &fx, &env)),
        Some(R::DecisionError)
    );
}

#[test]
fn torrent_indexer_rules() {
    let mut fx = Fx::new();
    fx.release.protocol = DownloadProtocol::Torrent;
    fx.release.torrent = Some(TorrentRelease {
        seeders: Some(2),
        peers: Some(5),
        ..Default::default()
    });
    let mut env = Env::new();
    env.indexers = vec![IndexerInfo {
        id: 1,
        tags: vec![],
        torrent: Some(TorrentIndexerSettings {
            minimum_seeders: 5,
            required_flags: vec![IndexerFlags::G_FREELEECH, IndexerFlags::G_INTERNAL],
        }),
        season_pack_max_single_episode_age_days: 0,
    }];
    assert_eq!(
        reason(verdict(&TorrentSeeding, &fx, &env)),
        Some(R::MinimumSeeders)
    );
    fx.release.torrent.as_mut().unwrap().seeders = Some(5);
    assert!(verdict(&TorrentSeeding, &fx, &env).is_accept());
    fx.release.torrent.as_mut().unwrap().seeders = None;
    assert!(
        verdict(&TorrentSeeding, &fx, &env).is_accept(),
        "unknown seeders pass"
    );

    assert_eq!(
        reason(verdict(&RequiredIndexerFlags, &fx, &env)),
        Some(R::RequiredFlags)
    );
    fx.release.indexer_flags = IndexerFlags::G_INTERNAL;
    assert!(
        verdict(&RequiredIndexerFlags, &fx, &env).is_accept(),
        "any one required flag is enough"
    );
}

#[test]
fn indexer_tags_must_overlap_movie_tags() {
    let mut fx = Fx::new();
    let mut env = Env::new();
    env.indexers = vec![IndexerInfo {
        id: 1,
        tags: vec![3],
        torrent: None,
        season_pack_max_single_episode_age_days: 0,
    }];
    assert_eq!(
        reason(verdict(&IndexerTag, &fx, &env)),
        Some(R::NoMatchingTag)
    );
    fx.movie.tags = vec![3, 9];
    assert!(verdict(&IndexerTag, &fx, &env).is_accept());
    env.indexers[0].tags.clear();
    fx.movie.tags.clear();
    assert!(
        verdict(&IndexerTag, &fx, &env).is_accept(),
        "untagged indexer takes everything"
    );
}

// ---- monitored, availability, search ---------------------------------------------

#[test]
fn unmonitored_movies_only_via_user_search() {
    let mut fx = Fx::new();
    let env = Env::new();
    fx.movie.monitored = false;
    assert_eq!(
        reason(verdict(&MonitoredMovie, &fx, &env)),
        Some(R::MovieNotMonitored)
    );
    assert!(verdict_with(&MonitoredMovie, &fx, &env, user_search(1)).is_accept());
    assert_eq!(
        reason(verdict_with(
            &MonitoredMovie,
            &fx,
            &env,
            scheduled_search(1)
        )),
        Some(R::MovieNotMonitored)
    );
}

#[test]
fn availability_follows_minimum_availability() {
    let mut fx = Fx::new();
    let mut env = Env::new();
    fx.movie.minimum_availability = MovieStatus::Released;
    assert_eq!(
        reason(verdict(&Availability, &fx, &env)),
        Some(R::Availability),
        "no dates at all"
    );
    assert!(verdict_with(&Availability, &fx, &env, user_search(1)).is_accept());

    fx.metadata.digital_release = Some(now() - Duration::days(2));
    assert!(verdict(&Availability, &fx, &env).is_accept());
    env.config.availability_delay = 5;
    assert_eq!(
        reason(verdict(&Availability, &fx, &env)),
        Some(R::Availability),
        "2 days of 5 elapsed"
    );
}

#[test]
fn search_must_be_for_the_same_movie() {
    let fx = Fx::new();
    let env = Env::new();
    assert!(verdict(&SearchMovie, &fx, &env).is_accept(), "rss");
    assert_eq!(
        reason(verdict_with(&SearchMovie, &fx, &env, user_search(2))),
        Some(R::WrongMovie)
    );
    assert!(verdict_with(&SearchMovie, &fx, &env, user_search(1)).is_accept());
}

#[test]
fn blocklist_and_blocked_indexer() {
    let fx = Fx::new();
    let mut env = Env::new();
    assert!(verdict(&Blocklist, &fx, &env).is_accept());
    env.blocklisted = true;
    assert_eq!(reason(verdict(&Blocklist, &fx, &env)), Some(R::Blocklisted));

    assert!(verdict(&BlockedIndexer, &fx, &env).is_accept());
    env.blocked_till = Some(now() + Duration::hours(1));
    assert_eq!(
        reason(verdict(&BlockedIndexer, &fx, &env)),
        Some(R::IndexerDisabled)
    );
    assert_eq!(BlockedIndexer.kind(), RejectionKind::Temporary);
}

// ---- profile and upgrades -----------------------------------------------------------

#[test]
fn quality_must_be_allowed_by_the_profile() {
    let env = Env::new();
    let sdtv = Fx::new().quality(Quality::SDTV);
    assert_eq!(
        reason(verdict(&QualityAllowedByProfile, &sdtv, &env)),
        Some(R::QualityNotWanted)
    );
    // A quality the profile does not list ranks at index 0 (SDTV, disallowed).
    let remux = Fx::new().quality(Quality::REMUX_2160P);
    assert_eq!(
        reason(verdict(&QualityAllowedByProfile, &remux, &env)),
        Some(R::QualityNotWanted)
    );
    assert!(verdict(&QualityAllowedByProfile, &Fx::new(), &env).is_accept());
}

#[test]
fn minimum_custom_format_score() {
    let mut fx = Fx::new();
    let env = Env::new();
    fx.profile.min_format_score = 10;
    fx.score = 5;
    assert_eq!(
        reason(verdict(&CustomFormatAllowedByProfile, &fx, &env)),
        Some(R::CustomFormatMinimumScore)
    );
    fx.score = 10;
    assert!(verdict(&CustomFormatAllowedByProfile, &fx, &env).is_accept());
}

#[test]
fn upgrade_allowed_blocks_when_profile_forbids_upgrades() {
    let env = Env::new();
    let mut fx = Fx::new().with_file(Quality::DVD);
    assert!(verdict(&UpgradeAllowed, &fx, &env).is_accept());
    fx.profile.upgrade_allowed = false;
    assert_eq!(
        reason(verdict(&UpgradeAllowed, &fx, &env)),
        Some(R::QualityUpgradesDisabled)
    );
    assert!(
        verdict(&UpgradeAllowed, &Fx::new(), &env).is_accept(),
        "no file"
    );
}

#[test]
fn upgrade_over_the_file_on_disk() {
    let env = Env::new();
    let wanted = Fx::new()
        .quality(Quality::WEBDL_1080P)
        .with_file(Quality::DVD);
    assert!(verdict(&UpgradeDisk, &wanted, &env).is_accept());

    let met = Fx::new()
        .quality(Quality::BLURAY_1080P)
        .with_file(Quality::WEBDL_1080P);
    assert_eq!(
        reason(verdict(&UpgradeDisk, &met, &env)),
        Some(R::DiskCutoffMet),
        "cutoff is WEB-DL 1080p"
    );

    let worse = Fx::new()
        .quality(Quality::DVD)
        .with_file(Quality::WEBDL_720P);
    assert_eq!(
        reason(verdict(&UpgradeDisk, &worse, &env)),
        Some(R::DiskHigherPreference)
    );

    assert!(
        verdict(&UpgradeDisk, &Fx::new(), &env).is_accept(),
        "no file"
    );
}

fn queue_entry(quality: Quality) -> QueueEntry {
    QueueEntry {
        quality: model(quality),
        custom_format_ids: vec![],
        failed_pending: false,
    }
}

#[test]
fn queue_blocks_equal_or_better_items_already_downloading() {
    let fx = Fx::new().quality(Quality::WEBDL_720P);
    let mut env = Env::new();
    assert!(verdict(&Queue, &fx, &env).is_accept(), "empty queue");

    env.queue = vec![queue_entry(Quality::WEBDL_1080P)];
    assert_eq!(reason(verdict(&Queue, &fx, &env)), Some(R::QueueCutoffMet));

    env.queue = vec![queue_entry(Quality::DVD)];
    assert!(
        verdict(&Queue, &fx, &env).is_accept(),
        "queued DVD is worth upgrading"
    );

    let mut failed = queue_entry(Quality::WEBDL_1080P);
    failed.failed_pending = true;
    env.queue = vec![failed];
    assert!(
        verdict(&Queue, &fx, &env).is_accept(),
        "failed items do not block"
    );
}

fn grab(quality: Quality, age: Duration) -> HistoryEntry {
    HistoryEntry {
        event: HistoryEvent::Grabbed,
        date: now() - age,
        quality: model(quality),
        custom_format_ids: vec![],
        download_id: Some("ABC".into()),
        source_title: "Some.Movie.2020.1080p.WEB-DL.x264-GRP".into(),
    }
}

#[test]
fn history_stops_re_grabbing_what_was_just_grabbed() {
    let fx = Fx::new().quality(Quality::BLURAY_1080P);
    let mut env = Env::new();
    env.history = vec![grab(Quality::WEBDL_1080P, Duration::hours(1))];
    assert_eq!(
        reason(verdict(&History, &fx, &env)),
        Some(R::HistoryRecentCutoffMet)
    );

    // An old grab with completed-download handling on is no longer a reason to wait.
    env.history = vec![grab(Quality::WEBDL_1080P, Duration::days(2))];
    assert!(verdict(&History, &fx, &env).is_accept());
    env.config.enable_completed_download_handling = false;
    assert_eq!(
        reason(verdict(&History, &fx, &env)),
        Some(R::HistoryCdhDisabledCutoffMet)
    );

    // Searches skip history entirely.
    assert!(verdict_with(&History, &fx, &env, user_search(1)).is_accept());

    // A recent grab of something worse than the cutoff still allows an upgrade.
    env.config.enable_completed_download_handling = true;
    env.history = vec![grab(Quality::DVD, Duration::hours(1))];
    assert!(verdict(&History, &fx, &env).is_accept());
}

#[test]
fn already_imported_same_name_or_hash() {
    let mut fx = Fx::new().with_file(Quality::BLURAY_1080P);
    let mut env = Env::new();
    let imported = |quality| HistoryEntry {
        event: HistoryEvent::DownloadFolderImported,
        date: now(),
        quality: model(quality),
        custom_format_ids: vec![],
        download_id: Some("ABC".into()),
        source_title: String::new(),
    };
    // Grabbed a WEB-DL, imported as something else (quality changed on import).
    env.history = vec![
        imported(Quality::BLURAY_1080P),
        grab(Quality::WEBDL_1080P, Duration::days(1)),
    ];
    assert_eq!(
        reason(verdict(&AlreadyImported, &fx, &env)),
        Some(R::AlreadyImportedSameName)
    );

    fx.release.title = "Another.Release.Name".into();
    assert!(verdict(&AlreadyImported, &fx, &env).is_accept());
    fx.release.protocol = DownloadProtocol::Torrent;
    fx.release.torrent = Some(TorrentRelease {
        info_hash: Some("abc".into()),
        ..Default::default()
    });
    assert_eq!(
        reason(verdict(&AlreadyImported, &fx, &env)),
        Some(R::AlreadyImportedSameHash)
    );

    // Same quality in and out: nothing changed, so allow it.
    env.history = vec![
        imported(Quality::WEBDL_1080P),
        grab(Quality::WEBDL_1080P, Duration::days(1)),
    ];
    assert!(verdict(&AlreadyImported, &fx, &env).is_accept());
}

fn proper(quality: Quality) -> QualityModel {
    QualityModel {
        quality,
        revision: Revision {
            version: 2,
            real: 0,
            is_repack: false,
        },
        ..Default::default()
    }
}

#[test]
fn propers_for_old_files_and_when_disabled() {
    let mut fx = Fx::new().with_file(Quality::WEBDL_1080P);
    fx.parsed.quality = proper(Quality::WEBDL_1080P);
    let mut env = Env::new();
    assert!(verdict(&Proper, &fx, &env).is_accept(), "file is a day old");

    fx.existing_file.as_mut().unwrap().date_added = now() - Duration::days(30);
    assert_eq!(
        reason(verdict(&Proper, &fx, &env)),
        Some(R::ProperForOldFile)
    );

    env.config.propers_and_repacks = ProperDownloadTypes::DoNotUpgrade;
    assert_eq!(
        reason(verdict(&Proper, &fx, &env)),
        Some(R::PropersDisabled)
    );
    env.config.propers_and_repacks = ProperDownloadTypes::DoNotPrefer;
    assert!(verdict(&Proper, &fx, &env).is_accept());
}

#[test]
fn repacks_need_a_matching_release_group() {
    let mut fx = Fx::new().with_file(Quality::WEBDL_1080P);
    let mut p = proper(Quality::WEBDL_1080P);
    p.revision.is_repack = true;
    fx.parsed.quality = p;
    let mut env = Env::new();
    assert!(verdict(&Repack, &fx, &env).is_accept(), "same group GRP");

    fx.parsed.release_group = Some("OTHER".into());
    assert_eq!(
        reason(verdict(&Repack, &fx, &env)),
        Some(R::RepackReleaseGroupDoesNotMatch)
    );
    fx.parsed.release_group = None;
    assert_eq!(
        reason(verdict(&Repack, &fx, &env)),
        Some(R::RepackUnknownReleaseGroup)
    );
    fx.parsed.release_group = Some("grp".into());
    assert!(verdict(&Repack, &fx, &env).is_accept(), "case-insensitive");

    env.config.propers_and_repacks = ProperDownloadTypes::DoNotUpgrade;
    assert_eq!(reason(verdict(&Repack, &fx, &env)), Some(R::RepackDisabled));
}

#[test]
fn pending_release_delays_pushed_ones() {
    let mut fx = Fx::new();
    let mut env = Env::new();
    env.pending = true;
    assert!(
        verdict(&Pending, &fx, &env).is_accept(),
        "rss never waits on pending"
    );
    fx.source = ReleaseSource::Search;
    assert_eq!(
        reason(verdict(&Pending, &fx, &env)),
        Some(R::MinimumAgeDelayPushed)
    );
    assert_eq!(Pending.kind(), RejectionKind::Temporary);
    assert!(verdict_with(&Pending, &fx, &env, user_search(1)).is_accept());
}

// ---- delay -------------------------------------------------------------------------

#[test]
fn delay_waits_for_the_configured_minutes() {
    let mut fx = Fx::new(); // 60 minutes old
    let mut env = Env::new();
    assert!(
        verdict(&Delay, &fx, &env).is_accept(),
        "no delay configured"
    );

    env.delay_profiles[0].usenet_delay = 120;
    assert_eq!(reason(verdict(&Delay, &fx, &env)), Some(R::MinimumAgeDelay));
    assert_eq!(Delay.kind(), RejectionKind::Temporary);
    assert!(verdict_with(&Delay, &fx, &env, user_search(1)).is_accept());

    fx.release.publish_date = now() - Duration::minutes(180);
    assert!(verdict(&Delay, &fx, &env).is_accept(), "old enough");
}

#[test]
fn delay_bypasses() {
    let mut env = Env::new();
    env.delay_profiles[0].usenet_delay = 120;

    // Best quality in the profile (Bluray 1080p) on the preferred protocol.
    env.delay_profiles[0].bypass_if_highest_quality = true;
    let best = Fx::new().quality(Quality::BLURAY_1080P);
    assert!(verdict(&Delay, &best, &env).is_accept());
    let not_best = Fx::new().quality(Quality::WEBDL_1080P);
    assert_eq!(
        reason(verdict(&Delay, &not_best, &env)),
        Some(R::MinimumAgeDelay)
    );

    // Custom-format score.
    env.delay_profiles[0].bypass_if_highest_quality = false;
    env.delay_profiles[0].bypass_if_above_custom_format_score = true;
    env.delay_profiles[0].minimum_custom_format_score = 100;
    let mut scored = Fx::new();
    scored.score = 100;
    assert!(verdict(&Delay, &scored, &env).is_accept());
    scored.score = 99;
    assert_eq!(
        reason(verdict(&Delay, &scored, &env)),
        Some(R::MinimumAgeDelay)
    );

    // A non-preferred protocol never bypasses.
    let mut torrent = Fx::new().quality(Quality::BLURAY_1080P);
    torrent.release.protocol = DownloadProtocol::Torrent;
    env.delay_profiles[0].torrent_delay = 120;
    env.delay_profiles[0].bypass_if_highest_quality = true;
    assert_eq!(
        reason(verdict(&Delay, &torrent, &env)),
        Some(R::MinimumAgeDelay)
    );
}

#[test]
fn delay_ends_when_an_older_release_is_already_pending() {
    let fx = Fx::new();
    let mut env = Env::new();
    env.delay_profiles[0].usenet_delay = 120;
    env.oldest_pending_minutes = Some(60.0);
    assert_eq!(reason(verdict(&Delay, &fx, &env)), Some(R::MinimumAgeDelay));
    env.oldest_pending_minutes = Some(121.0);
    assert!(verdict(&Delay, &fx, &env).is_accept());
}

#[test]
fn a_proper_that_upgrades_the_file_skips_the_delay() {
    let mut fx = Fx::new().with_file(Quality::WEBDL_1080P);
    fx.parsed.quality = proper(Quality::WEBDL_1080P);
    let mut env = Env::new();
    env.delay_profiles[0].usenet_delay = 120;
    assert!(verdict(&Delay, &fx, &env).is_accept());
    env.config.propers_and_repacks = ProperDownloadTypes::DoNotPrefer;
    assert_eq!(
        reason(verdict(&Delay, &fx, &env)),
        Some(R::MinimumAgeDelay),
        "propers not preferred"
    );
}

// ---- free space ----------------------------------------------------------------------

#[test]
fn free_space() {
    let fx = Fx::new(); // 4 GB
    let mut env = Env::new();
    const GB: u64 = 1024 * 1024 * 1024;
    assert!(verdict(&FreeSpace, &fx, &env).is_accept(), "unknown space");
    env.free_space = Some(3 * GB);
    assert_eq!(
        reason(verdict(&FreeSpace, &fx, &env)),
        Some(R::MinimumFreeSpace)
    );
    env.free_space = Some(4 * GB + 50 * 1024 * 1024); // 50 MB left, 100 MB wanted
    assert_eq!(
        reason(verdict(&FreeSpace, &fx, &env)),
        Some(R::MinimumFreeSpace)
    );
    env.free_space = Some(10 * GB);
    assert!(verdict(&FreeSpace, &fx, &env).is_accept());
    env.free_space = Some(GB);
    env.config.skip_free_space_check_when_importing = true;
    assert!(verdict(&FreeSpace, &fx, &env).is_accept());
}

// ---- engine --------------------------------------------------------------------------

#[test]
fn engine_reports_every_rejection_in_the_first_failing_group() {
    let mut fx = Fx::new();
    fx.movie.monitored = false;
    let mut env = Env::new();
    env.blocklisted = true;
    let d = DecisionEngine::default().decide(&fx.subject(), &DecisionInfo::default(), &env);
    let reasons: Vec<_> = d.rejections.iter().map(|r| r.reason).collect();
    assert!(reasons.contains(&R::MovieNotMonitored));
    assert!(reasons.contains(&R::Blocklisted));
    assert!(d.rejected());
    assert!(!d.temporarily_rejected());
}

#[test]
fn engine_skips_the_disk_group_when_earlier_specs_reject() {
    let mut fx = Fx::new();
    fx.movie.monitored = false;
    let mut env = Env::new();
    env.free_space = Some(1); // would fail FreeSpace
    let d = DecisionEngine::default().decide(&fx.subject(), &DecisionInfo::default(), &env);
    assert!(d.rejections.iter().all(|r| r.reason != R::MinimumFreeSpace));

    fx.movie.monitored = true;
    let d = DecisionEngine::default().decide(&fx.subject(), &DecisionInfo::default(), &env);
    assert_eq!(d.rejections.len(), 1);
    assert_eq!(
        d.rejections[0].reason,
        R::MinimumFreeSpace,
        "disk group runs once the rest passes"
    );
}

#[test]
fn engine_marks_temporary_rejections() {
    let fx = Fx::new();
    let mut env = Env::new();
    env.delay_profiles[0].usenet_delay = 120;
    let d = DecisionEngine::default().decide(&fx.subject(), &DecisionInfo::default(), &env);
    assert_eq!(d.rejections.len(), 1);
    assert!(d.temporarily_rejected());
    assert!(!d.rejected());
    assert!(!d.approved());
}

// ---- ranking -------------------------------------------------------------------------

use crate::compare::{compare, sort_best_first};
use std::cmp::Ordering::{Equal, Greater, Less};

fn torrent(mut fx: Fx, seeders: i32, peers: i32) -> Fx {
    fx.release.protocol = DownloadProtocol::Torrent;
    fx.release.torrent = Some(TorrentRelease {
        seeders: Some(seeders),
        peers: Some(peers),
        ..Default::default()
    });
    fx
}

#[test]
fn ranking_quality_comes_first_then_revision_then_score() {
    let env = Env::new();
    let web = Fx::new();
    let bluray = Fx::new().quality(Quality::BLURAY_1080P);
    assert_eq!(compare(&bluray.subject(), &web.subject(), &env), Greater);

    let mut p = Fx::new();
    p.parsed.quality = proper(Quality::WEBDL_1080P);
    assert_eq!(compare(&p.subject(), &web.subject(), &env), Greater);
    let mut env2 = Env::new();
    env2.config.propers_and_repacks = ProperDownloadTypes::DoNotPrefer;
    assert_eq!(
        compare(&p.subject(), &web.subject(), &env2),
        Equal,
        "revision ignored"
    );

    let mut scored = Fx::new();
    scored.score = 50;
    assert_eq!(compare(&scored.subject(), &web.subject(), &env), Greater);
    // Quality beats score.
    let mut cheap_bluray = Fx::new().quality(Quality::BLURAY_1080P);
    cheap_bluray.score = -100;
    assert_eq!(
        compare(&cheap_bluray.subject(), &scored.subject(), &env),
        Greater
    );
}

#[test]
fn ranking_prefers_the_delay_profile_protocol_then_indexer_priority() {
    let env = Env::new(); // prefers usenet
    let usenet = Fx::new();
    let torrent = torrent(Fx::new(), 100, 100);
    assert_eq!(
        compare(&usenet.subject(), &torrent.subject(), &env),
        Greater
    );

    let mut low = Fx::new();
    low.release.indexer_priority = 10;
    let mut high = Fx::new();
    high.release.indexer_priority = 40;
    assert_eq!(
        compare(&low.subject(), &high.subject(), &env),
        Greater,
        "lower number wins"
    );
}

#[test]
fn ranking_indexer_flags_only_when_enabled() {
    let mut env = Env::new();
    let mut free = Fx::new();
    free.release.indexer_flags = IndexerFlags::G_FREELEECH;
    let plain = Fx::new();
    assert_eq!(compare(&free.subject(), &plain.subject(), &env), Equal);
    env.config.prefer_indexer_flags = true;
    assert_eq!(compare(&free.subject(), &plain.subject(), &env), Greater);
}

#[test]
fn ranking_torrents_by_seeder_and_peer_magnitude() {
    let mut env = Env::new();
    env.delay_profiles[0].preferred_protocol = DownloadProtocol::Torrent;
    let many = torrent(Fx::new(), 1000, 1200);
    let few = torrent(Fx::new(), 12, 20);
    assert_eq!(compare(&many.subject(), &few.subject(), &env), Greater);
    // 90 and 110 seeders fall in the same log bucket, so peers decide.
    let a = torrent(Fx::new(), 90, 5);
    let b = torrent(Fx::new(), 110, 5);
    assert_eq!(compare(&a.subject(), &b.subject(), &env), Equal);
    let c = torrent(Fx::new(), 90, 5000);
    assert_eq!(compare(&c.subject(), &b.subject(), &env), Greater);
}

#[test]
fn ranking_usenet_by_age_bucket() {
    let env = Env::new();
    let mut fresh = Fx::new();
    fresh.release.publish_date = now() - Duration::minutes(10);
    let mut day = Fx::new();
    day.release.publish_date = now() - Duration::hours(10);
    let mut week = Fx::new();
    week.release.publish_date = now() - Duration::days(5);
    let mut old = Fx::new();
    old.release.publish_date = now() - Duration::days(400);
    assert_eq!(compare(&fresh.subject(), &day.subject(), &env), Greater);
    assert_eq!(compare(&day.subject(), &week.subject(), &env), Greater);
    assert_eq!(compare(&week.subject(), &old.subject(), &env), Greater);
}

#[test]
fn ranking_size_prefers_the_preferred_size_or_the_biggest() {
    const GB: u64 = 1024 * 1024 * 1024;
    // Preferred 95 MB/min * 120 min = ~11.1 GB.
    let env = Env::new();
    let mut near = Fx::new();
    near.release.size = 11 * GB;
    let mut far = Fx::new();
    far.release.size = 3 * GB;
    assert_eq!(compare(&near.subject(), &far.subject(), &env), Greater);

    // Bluray 1080p has no preferred size: bigger wins.
    let mut big = Fx::new().quality(Quality::BLURAY_1080P);
    big.release.size = 30 * GB;
    let mut small = Fx::new().quality(Quality::BLURAY_1080P);
    small.release.size = 10 * GB;
    assert_eq!(compare(&big.subject(), &small.subject(), &env), Greater);

    // Differences under 200 MB do not matter.
    let mut a = Fx::new().quality(Quality::BLURAY_1080P);
    a.release.size = 10 * GB;
    let mut b = Fx::new().quality(Quality::BLURAY_1080P);
    b.release.size = 10 * GB + 100 * 1024 * 1024;
    assert_eq!(compare(&a.subject(), &b.subject(), &env), Equal);
}

#[test]
fn sorting_puts_the_best_first_and_is_stable() {
    let env = Env::new();
    let dvd = Fx::new().quality(Quality::DVD);
    let web = Fx::new();
    let bluray = Fx::new().quality(Quality::BLURAY_1080P);
    let web2 = Fx::new();
    let (s_dvd, s_web, s_bluray, s_web2) = (
        dvd.subject(),
        web.subject(),
        bluray.subject(),
        web2.subject(),
    );
    let mut list = vec![&s_dvd, &s_web, &s_bluray, &s_web2];
    sort_best_first(&mut list, &env);
    let ids: Vec<_> = list.iter().map(|s| s.parsed.quality.quality.id).collect();
    assert_eq!(
        ids,
        [
            Quality::BLURAY_1080P.id,
            Quality::WEBDL_1080P.id,
            Quality::WEBDL_1080P.id,
            Quality::DVD.id
        ]
    );
    // Equal releases keep input order.
    assert!(std::ptr::eq(list[1], &s_web) && std::ptr::eq(list[2], &s_web2));
    assert_eq!(compare(&s_web, &s_web2, &env), Equal);
    assert_eq!(compare(&s_dvd, &s_web, &env), Less);
}
