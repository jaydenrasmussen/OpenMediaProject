//! The TV specifications, ported from Sonarr's decision engine (GPL-3.0).

use std::{cmp::Ordering, sync::LazyLock};

use chrono::Duration;
use omp_core::{release::DownloadProtocol, tv::SeriesType};
use omp_parser::re::Re;
use omp_quality::{
    release_profile::term_matches,
    upgrade::{
        Candidate, ProperDownloadTypes, UpgradeRejectReason, cutoff_not_met, is_revision_upgrade,
        is_upgradable, is_upgrade_allowed,
    },
};

use super::model::{EpisodeFileView, EpisodeSubject, TvDecisionData, TvDecisionInfo, TvSearchKind};
use crate::{
    model::{HistoryEvent, ReleaseSource},
    reason::{Family, RejectionKind, RejectionReason as R, Verdict},
    specs::{Priority, megabytes, profile_item_name, size_suffix, upgrade_verdict},
};

pub trait TvSpecification: Send + Sync {
    fn name(&self) -> &'static str;
    fn priority(&self) -> Priority {
        Priority::Default
    }
    fn kind(&self) -> RejectionKind {
        RejectionKind::Permanent
    }
    fn evaluate(
        &self,
        s: &EpisodeSubject<'_>,
        info: &TvDecisionInfo,
        env: &dyn TvDecisionData,
    ) -> Verdict;
}

type Env<'a> = &'a dyn TvDecisionData;

fn new_candidate<'a>(s: &'a EpisodeSubject<'_>) -> Candidate<'a> {
    Candidate {
        quality: &s.parsed.quality,
        format_ids: s.custom_format_ids,
    }
}

fn file_candidate(f: &EpisodeFileView) -> Candidate<'_> {
    Candidate {
        quality: &f.quality,
        format_ids: &f.custom_format_ids,
    }
}

macro_rules! spec {
    ($name:ident, $label:literal, $($rest:item),* $(,)?) => {
        pub struct $name;
        impl TvSpecification for $name {
            fn name(&self) -> &'static str { $label }
            $($rest)*
        }
    };
}

spec!(
    MinimumAge,
    "MinimumAge",
    fn kind(&self) -> RejectionKind {
        RejectionKind::Temporary
    },
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        if s.release.protocol != DownloadProtocol::Usenet {
            return Verdict::Accept;
        }
        let minimum = env.config().minimum_age;
        if minimum == 0 {
            return Verdict::Accept;
        }
        let age = s.release.age_minutes(env.now());
        if age < f64::from(minimum) {
            return Verdict::reject(
                R::MinimumAge,
                format!("Only {age:.1} minutes old, minimum age is {minimum} minutes"),
            );
        }
        Verdict::Accept
    }
);

spec!(
    Retention,
    "Retention",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        if s.release.protocol != DownloadProtocol::Usenet {
            return Verdict::Accept;
        }
        let retention = i64::from(env.config().retention);
        if retention > 0 && s.release.age_days(env.now()) > retention {
            return Verdict::reject(R::MaximumAge, "Older than configured retention");
        }
        Verdict::Accept
    }
);

spec!(
    AcceptableSize,
    "AcceptableSize",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        let size = s.release.size as i64;
        if s.parsed.special || size == 0 {
            return Verdict::Accept;
        }
        let mut series_runtime = i64::from(s.series.runtime);
        if series_runtime == 0
            && let (Some(first), Some(start)) = (
                s.series.first_season,
                s.series.first_season_start.or(Some(s.release.publish_date)),
            )
            && s.episodes.first().is_some_and(|e| e.season_number == first)
        {
            let grace_end = start + Duration::hours(24);
            if s.episodes
                .iter()
                .all(|e| e.air_date_utc.is_none_or(|d| d < grace_end))
            {
                series_runtime = 45;
            }
        }
        let runtime: i64 = s
            .episodes
            .iter()
            .map(|e| {
                if e.runtime > 0 {
                    i64::from(e.runtime)
                } else {
                    series_runtime
                }
            })
            .sum();
        if runtime == 0 {
            return Verdict::reject(
                R::UnknownRuntime,
                "Runtime of all episodes is 0, unable to validate size until it is available",
            );
        }
        let Some(def) = env.quality_definitions().get(s.parsed.quality.quality) else {
            return Verdict::Accept;
        };
        let what = if s.episodes.len() == 1 {
            format!("{runtime}min")
        } else {
            format!("{} episodes totalling {runtime}min", s.episodes.len())
        };
        if let Some(min) = def.min_size {
            let min_size = megabytes(min) * runtime;
            if size < min_size {
                return Verdict::reject(
                    R::BelowMinimumSize,
                    format!(
                        "{} is smaller than minimum allowed {} (for {what})",
                        size_suffix(size),
                        size_suffix(min_size)
                    ),
                );
            }
        }
        if let Some(max) = def.max_size.filter(|m| *m != 0.0) {
            let max_size = megabytes(max) * runtime;
            if size > max_size {
                return Verdict::reject(
                    R::AboveMaximumSize,
                    format!(
                        "{} is larger than maximum allowed {} (for {what})",
                        size_suffix(size),
                        size_suffix(max_size)
                    ),
                );
            }
        }
        Verdict::Accept
    }
);

spec!(
    MaximumSize,
    "MaximumSize",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        let maximum = env.config().maximum_size * 1024 * 1024;
        let size = s.release.size as i64;
        if maximum == 0 || size == 0 {
            return Verdict::Accept;
        }
        if size > maximum {
            return Verdict::reject(
                R::MaximumSizeExceeded,
                format!(
                    "{} is too big, maximum size is {} (Settings->Indexers->Maximum Size)",
                    size_suffix(size),
                    size_suffix(maximum)
                ),
            );
        }
        Verdict::Accept
    }
);

spec!(
    NotSample,
    "NotSample",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, _: Env) -> Verdict {
        if s.release.title.to_lowercase().contains("sample") && s.release.size < 70 * 1024 * 1024 {
            return Verdict::reject(R::Sample, "Sample");
        }
        Verdict::Accept
    }
);

static DISC_PATTERNS: LazyLock<[Re; 3]> = LazyLock::new(|| {
    [
        Re::ci(
            r"(?:dis[ck])(?:[-_. ]\d+[-_. ])(?:(?:(?:480|720|1080|2160)[ip]|)[-_. ])?(?:Blu\-?ray)",
        ),
        Re::ci(r"(?:(?:480|720|1080|2160)[ip]|)[-_. ](?:full)[-_. ](?:Blu\-?ray)"),
        Re::ci(r"(?:\d?x?M?DVD-?[R59])(?:[ ._]|$)"),
    ]
});

spec!(
    RawDisk,
    "RawDisk",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, _: Env) -> Verdict {
        if DISC_PATTERNS.iter().any(|r| r.is_match(&s.release.title)) {
            return Verdict::reject(R::Raw, "Raw Bluray/DVD release");
        }
        match s
            .release
            .container
            .as_deref()
            .map(str::to_lowercase)
            .as_deref()
        {
            Some("vob" | "iso") => Verdict::reject(R::Raw, "Raw DVD release"),
            Some("m2ts") => Verdict::reject(R::Raw, "Raw Bluray release"),
            _ => Verdict::Accept,
        }
    }
);

spec!(
    Protocol,
    "Protocol",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        let profile = env.delay_profile_for(&s.series.tags);
        match s.release.protocol {
            DownloadProtocol::Usenet if !profile.enable_usenet => {
                Verdict::reject(R::ProtocolDisabled, "Usenet is not enabled for this series")
            }
            DownloadProtocol::Torrent if !profile.enable_torrent => Verdict::reject(
                R::ProtocolDisabled,
                "Torrent is not enabled for this series",
            ),
            _ => Verdict::Accept,
        }
    }
);

spec!(
    QualityAllowedByProfile,
    "QualityAllowedByProfile",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, _: Env) -> Verdict {
        let quality = s.parsed.quality.quality;
        let index = s.profile.index_of(quality, false).index;
        match s.profile.items.get(index) {
            Some(item) if !item.allowed() => Verdict::reject(
                R::QualityNotWanted,
                format!("{quality} is not wanted in profile"),
            ),
            _ => Verdict::Accept,
        }
    }
);

spec!(
    CustomFormatAllowedByProfile,
    "CustomFormatAllowedByProfile",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, _: Env) -> Verdict {
        let minimum = s.profile.min_format_score;
        if s.custom_format_score < minimum {
            return Verdict::reject(
                R::CustomFormatMinimumScore,
                format!(
                    "Custom Formats have score {} below Series profile minimum {minimum}",
                    s.custom_format_score
                ),
            );
        }
        Verdict::Accept
    }
);

spec!(
    ReleaseRestrictions,
    "ReleaseRestrictions",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        let title = &s.release.title;
        let profiles = env.release_profiles(&s.series.tags, s.release.indexer_id);
        let found = |terms: &[String]| -> Result<Vec<String>, String> {
            let mut out = Vec::new();
            for t in terms {
                if term_matches(t, title)? {
                    out.push(t.clone());
                }
            }
            Ok(out)
        };
        for p in profiles.iter().filter(|p| !p.required.is_empty()) {
            match found(&p.required) {
                Err(e) => {
                    return Verdict::reject(R::DecisionError, format!("ReleaseRestrictions: {e}"));
                }
                Ok(f) if f.is_empty() => {
                    return Verdict::reject(
                        R::MustContainMissing,
                        format!(
                            "Does not contain one of the required terms: {}",
                            p.required.join(", ")
                        ),
                    );
                }
                Ok(_) => {}
            }
        }
        for p in profiles.iter().filter(|p| !p.ignored.is_empty()) {
            match found(&p.ignored) {
                Err(e) => {
                    return Verdict::reject(R::DecisionError, format!("ReleaseRestrictions: {e}"));
                }
                Ok(f) if !f.is_empty() => {
                    return Verdict::reject(
                        R::MustNotContainPresent,
                        format!("Contains these ignored terms: {}", f.join(", ")),
                    );
                }
                Ok(_) => {}
            }
        }
        Verdict::Accept
    }
);

spec!(
    TorrentSeeding,
    "TorrentSeeding",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        if s.release.torrent.is_none() || s.release.indexer_id == 0 {
            return Verdict::Accept;
        }
        let Some(settings) = env.indexer(s.release.indexer_id).and_then(|i| i.torrent) else {
            return Verdict::Accept;
        };
        match s.release.seeders() {
            Some(n) if n < settings.minimum_seeders => Verdict::reject(
                R::MinimumSeeders,
                format!(
                    "Not enough seeders: {n}. Minimum seeders: {}",
                    settings.minimum_seeders
                ),
            ),
            _ => Verdict::Accept,
        }
    }
);

spec!(
    IndexerTag,
    "IndexerTag",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        if s.release.indexer_id == 0 {
            return Verdict::Accept;
        }
        let Some(indexer) = env.indexer(s.release.indexer_id) else {
            return Verdict::Accept;
        };
        if !indexer.tags.is_empty() && !indexer.tags.iter().any(|t| s.series.tags.contains(t)) {
            return Verdict::reject(
                R::NoMatchingTag,
                "Series tags do not match any of the indexer tags",
            );
        }
        Verdict::Accept
    }
);

spec!(
    MonitoredEpisode,
    "MonitoredEpisode",
    fn evaluate(&self, s: &EpisodeSubject<'_>, info: &TvDecisionInfo, _: Env) -> Verdict {
        if info.search.as_ref().is_some_and(|c| !c.monitored_only) {
            return Verdict::Accept;
        }
        if !s.series.monitored {
            return Verdict::reject(R::SeriesNotMonitored, "Series is not monitored");
        }
        let monitored = s.episodes.iter().filter(|e| e.monitored).count();
        if monitored == s.episodes.len() {
            return Verdict::Accept;
        }
        if s.episodes.len() == 1 {
            return Verdict::reject(R::EpisodeNotMonitored, "Episode is not monitored");
        }
        Verdict::reject(
            R::EpisodeNotMonitored,
            "One or more episodes is not monitored",
        )
    }
);

spec!(
    MultiSeason,
    "MultiSeason",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, _: Env) -> Verdict {
        if s.parsed.is_multi_season() {
            Verdict::reject(R::MultiSeason, "Multi-season releases are not supported")
        } else {
            Verdict::Accept
        }
    }
);

spec!(
    SplitEpisode,
    "SplitEpisode",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, _: Env) -> Verdict {
        if s.parsed.is_split_episode {
            Verdict::reject(R::SplitEpisode, "Split episode releases are not supported")
        } else {
            Verdict::Accept
        }
    }
);

spec!(
    FullSeason,
    "FullSeason",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        if s.parsed.full_season {
            let limit = env.now() + Duration::hours(24);
            if s.episodes
                .iter()
                .any(|e| e.air_date_utc.is_none_or(|d| d > limit))
            {
                return Verdict::reject(
                    R::FullSeasonNotAired,
                    "Full season release rejected. All episodes haven't aired yet.",
                );
            }
        }
        Verdict::Accept
    }
);

spec!(
    SameEpisodesGrab,
    "SameEpisodesGrab",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, _: Env) -> Verdict {
        let ids: Vec<i64> = s.episodes.iter().map(|e| e.id).collect();
        for f in s.files {
            if f.episode_ids.iter().any(|id| !ids.contains(id)) {
                return Verdict::reject(
                    R::ExistingFileHasMoreEpisodes,
                    "Episode file on disk contains more episodes than this release contains",
                );
            }
        }
        Verdict::Accept
    }
);

spec!(
    Blocklist,
    "Blocklist",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        if env.is_blocklisted(s.series.id, s.release) {
            Verdict::reject(R::Blocklisted, "Release is blocklisted")
        } else {
            Verdict::Accept
        }
    }
);

spec!(
    BlockedIndexer,
    "BlockedIndexer",
    fn kind(&self) -> RejectionKind {
        RejectionKind::Temporary
    },
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        if let Some(till) = env.indexer_blocked_till(s.release.indexer_id) {
            return Verdict::reject(
                R::IndexerDisabled,
                format!(
                    "Indexer {} is blocked till {till} due to failures, cannot grab release.",
                    s.release.indexer
                ),
            );
        }
        Verdict::Accept
    }
);

spec!(
    UpgradeAllowed,
    "UpgradeAllowed",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, _: Env) -> Verdict {
        for f in s.files {
            if !is_upgrade_allowed(s.profile, file_candidate(f), new_candidate(s)) {
                return Verdict::reject(
                    R::QualityUpgradesDisabled,
                    "Existing file and the Quality profile does not allow upgrades",
                );
            }
        }
        Verdict::Accept
    }
);

fn disk_verdict(s: &EpisodeSubject<'_>, f: &EpisodeFileView, env: Env) -> Verdict {
    let profile = s.profile;
    if !cutoff_not_met(
        profile,
        &f.quality,
        &f.custom_format_ids,
        Some(&s.parsed.quality),
    ) {
        let cutoff = if profile.upgrade_allowed {
            profile.cutoff
        } else {
            profile.first_allowed_quality().map_or(0, |q| q.id)
        };
        return Verdict::reject(
            R::DiskCutoffMet,
            format!(
                "Existing file meets cutoff: {}",
                profile_item_name(profile, cutoff)
            ),
        );
    }
    let reason = is_upgradable(
        profile,
        env.config().propers_and_repacks,
        file_candidate(f),
        new_candidate(s),
    );
    upgrade_verdict(
        Family::Disk,
        reason,
        profile,
        &f.quality,
        &f.custom_format_ids,
        "",
    )
}

spec!(
    UpgradeDisk,
    "UpgradeDisk",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        if s.parsed.full_season {
            // A season pack is worth it when anything in it is missing or can be upgraded.
            let total = s.episodes.len();
            if total == 0 {
                return Verdict::Accept;
            }
            let missing = s.episodes.iter().filter(|e| e.file_id.is_none()).count();
            if s.files.is_empty() {
                return Verdict::Accept;
            }
            let upgradable = s
                .files
                .iter()
                .filter(|f| {
                    cutoff_not_met(
                        s.profile,
                        &f.quality,
                        &f.custom_format_ids,
                        Some(&s.parsed.quality),
                    ) && is_upgradable(
                        s.profile,
                        env.config().propers_and_repacks,
                        file_candidate(f),
                        new_candidate(s),
                    ) == UpgradeRejectReason::None
                })
                .count();
            if missing + upgradable > 0 {
                return Verdict::Accept;
            }
            return Verdict::reject(
                R::DiskHigherPreference,
                format!("Season pack does not meet the upgrade criteria. Upgradable: 0/{total}"),
            );
        }
        for f in s.files {
            let v = disk_verdict(s, f, env);
            if !v.is_accept() {
                return v;
            }
        }
        Verdict::Accept
    }
);

spec!(
    AnimeVersionUpgrade,
    "AnimeVersionUpgrade",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        if s.series.series_type != SeriesType::Anime
            || env.config().propers_and_repacks == ProperDownloadTypes::DoNotPrefer
        {
            return Verdict::Accept;
        }
        let blank = |g: &Option<String>| g.as_deref().is_none_or(|g| g.trim().is_empty());
        for f in s.files {
            if is_revision_upgrade(&f.quality, &s.parsed.quality) {
                if blank(&f.release_group) {
                    return Verdict::reject(
                        R::UnknownReleaseGroup,
                        "Existing release group is unknown",
                    );
                }
                if blank(&s.parsed.release_group) {
                    return Verdict::reject(R::UnknownReleaseGroup, "Release group is unknown");
                }
                let (a, b) = (
                    s.parsed.release_group.as_deref().unwrap_or_default(),
                    f.release_group.as_deref().unwrap_or_default(),
                );
                if a != b {
                    return Verdict::reject(
                        R::ReleaseGroupDoesNotMatch,
                        format!("{a} does not match existing release group {b}"),
                    );
                }
            }
        }
        Verdict::Accept
    }
);

spec!(
    Queue,
    "Queue",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        let propers = env.config().propers_and_repacks;
        let ids: Vec<i64> = s.episodes.iter().map(|e| e.id).collect();
        for item in env.queue_for_series(s.series.id) {
            if item.failed_pending || !item.episode_ids.iter().any(|id| ids.contains(id)) {
                continue;
            }
            if !cutoff_not_met(
                s.profile,
                &item.quality,
                &item.custom_format_ids,
                Some(&s.parsed.quality),
            ) {
                return Verdict::reject(
                    R::QueueCutoffMet,
                    format!(
                        "Release in queue already meets cutoff: {}",
                        item.quality.quality
                    ),
                );
            }
            let reason = is_upgradable(
                s.profile,
                propers,
                Candidate {
                    quality: &item.quality,
                    format_ids: &item.custom_format_ids,
                },
                new_candidate(s),
            );
            if reason != UpgradeRejectReason::None {
                return upgrade_verdict(
                    Family::Queue,
                    reason,
                    s.profile,
                    &item.quality,
                    &item.custom_format_ids,
                    "",
                );
            }
            if is_revision_upgrade(&item.quality, &s.parsed.quality)
                && propers == ProperDownloadTypes::DoNotUpgrade
            {
                return Verdict::reject(R::QueuePropersDisabled, "Proper downloading is disabled");
            }
        }
        Verdict::Accept
    }
);

spec!(
    AlreadyImported,
    "AlreadyImported",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        if !env.config().enable_completed_download_handling {
            return Verdict::Accept;
        }
        for e in s.episodes.iter().filter(|e| e.file_id.is_some()) {
            let history = env.history_for_episode(e.id);
            let Some(grabbed) = history.iter().find(|h| h.event == HistoryEvent::Grabbed) else {
                continue;
            };
            let Some(imported) = history.iter().find(|h| {
                h.event == HistoryEvent::DownloadFolderImported
                    && h.download_id == grabbed.download_id
            }) else {
                continue;
            };
            if grabbed.quality == imported.quality {
                continue;
            }
            if let (Some(t), Some(id)) = (&s.release.torrent, &grabbed.download_id)
                && t.info_hash
                    .as_deref()
                    .is_some_and(|h| h.to_uppercase() == *id)
            {
                return Verdict::reject(
                    R::AlreadyImportedSameHash,
                    "Has same torrent hash as a grabbed and imported release",
                );
            }
            if s.release.title.to_lowercase() == grabbed.source_title.to_lowercase() {
                return Verdict::reject(
                    R::AlreadyImportedSameName,
                    "Has same release name as a grabbed and imported release",
                );
            }
        }
        Verdict::Accept
    }
);

spec!(
    Proper,
    "Proper",
    fn evaluate(&self, s: &EpisodeSubject<'_>, info: &TvDecisionInfo, env: Env) -> Verdict {
        if info.search.is_some() {
            return Verdict::Accept;
        }
        let propers = env.config().propers_and_repacks;
        if propers == ProperDownloadTypes::DoNotPrefer {
            return Verdict::Accept;
        }
        for f in s.files {
            if is_revision_upgrade(&f.quality, &s.parsed.quality) {
                if propers == ProperDownloadTypes::DoNotUpgrade {
                    return Verdict::reject(R::PropersDisabled, "Proper downloading is disabled");
                }
                let today = env
                    .now()
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .map(|d| d.and_utc())
                    .unwrap_or_else(|| env.now());
                if f.date_added < today - Duration::days(7) {
                    return Verdict::reject(R::ProperForOldFile, "Proper for old file");
                }
            }
        }
        Verdict::Accept
    }
);

spec!(
    Repack,
    "Repack",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        let propers = env.config().propers_and_repacks;
        if !s.parsed.quality.revision.is_repack || propers == ProperDownloadTypes::DoNotPrefer {
            return Verdict::Accept;
        }
        let blank = |g: &Option<String>| g.as_deref().is_none_or(|g| g.trim().is_empty());
        for f in s.files {
            if !is_revision_upgrade(&f.quality, &s.parsed.quality) {
                continue;
            }
            if propers == ProperDownloadTypes::DoNotUpgrade {
                return Verdict::reject(R::RepackDisabled, "Repack downloading is disabled");
            }
            if blank(&f.release_group) {
                return Verdict::reject(
                    R::RepackUnknownReleaseGroup,
                    "Unable to determine release group for the existing file",
                );
            }
            if blank(&s.parsed.release_group) {
                return Verdict::reject(
                    R::RepackUnknownReleaseGroup,
                    "Unable to determine release group for this release",
                );
            }
            let (a, b) = (
                s.parsed.release_group.as_deref().unwrap_or_default(),
                f.release_group.as_deref().unwrap_or_default(),
            );
            if a.to_lowercase() != b.to_lowercase() {
                return Verdict::reject(
                    R::RepackReleaseGroupDoesNotMatch,
                    format!(
                        "Release is a repack for a different release group. Release Group: {a}. File release group: {b}"
                    ),
                );
            }
        }
        Verdict::Accept
    }
);

spec!(
    History,
    "History",
    fn evaluate(&self, s: &EpisodeSubject<'_>, info: &TvDecisionInfo, env: Env) -> Verdict {
        if info.search.is_some() {
            return Verdict::Accept;
        }
        let cdh = env.config().enable_completed_download_handling;
        for e in s.episodes {
            let history = env.history_for_episode(e.id);
            let Some(recent) = history.first().filter(|h| h.event == HistoryEvent::Grabbed) else {
                continue;
            };
            let is_recent = recent.date > env.now() - Duration::hours(12);
            if !is_recent && cdh {
                continue;
            }
            if !cutoff_not_met(
                s.profile,
                &recent.quality,
                &recent.custom_format_ids,
                Some(&s.parsed.quality),
            ) {
                return if is_recent {
                    Verdict::reject(
                        R::HistoryRecentCutoffMet,
                        format!(
                            "Recent grab event in history already meets cutoff: {}",
                            recent.quality.quality
                        ),
                    )
                } else {
                    Verdict::reject(
                        R::HistoryCdhDisabledCutoffMet,
                        format!(
                            "CDH is disabled and grab event in history already meets cutoff: {}",
                            recent.quality.quality
                        ),
                    )
                };
            }
            let reason = is_upgradable(
                s.profile,
                env.config().propers_and_repacks,
                Candidate {
                    quality: &recent.quality,
                    format_ids: &recent.custom_format_ids,
                },
                new_candidate(s),
            );
            let prefix = if is_recent {
                "Recent"
            } else {
                "CDH is disabled and"
            };
            let v = upgrade_verdict(
                Family::History,
                reason,
                s.profile,
                &recent.quality,
                &recent.custom_format_ids,
                prefix,
            );
            if !v.is_accept() {
                return v;
            }
        }
        Verdict::Accept
    }
);

spec!(
    Pending,
    "Pending",
    fn kind(&self) -> RejectionKind {
        RejectionKind::Temporary
    },
    fn evaluate(&self, s: &EpisodeSubject<'_>, info: &TvDecisionInfo, env: Env) -> Verdict {
        if s.source == ReleaseSource::Rss || info.user_invoked_search() {
            return Verdict::Accept;
        }
        let ids: Vec<i64> = s.episodes.iter().map(|e| e.id).collect();
        if env.has_pending_release(s.series.id, &ids) {
            return Verdict::reject(
                R::MinimumAgeDelayPushed,
                "Release containing at least one matching episode is already pending, delaying pushed release",
            );
        }
        Verdict::Accept
    }
);

spec!(
    Delay,
    "Delay",
    fn kind(&self) -> RejectionKind {
        RejectionKind::Temporary
    },
    fn evaluate(&self, s: &EpisodeSubject<'_>, info: &TvDecisionInfo, env: Env) -> Verdict {
        if info.user_invoked_search() {
            return Verdict::Accept;
        }
        let dp = env.delay_profile_for(&s.series.tags);
        let delay = dp.protocol_delay(s.release.protocol);
        let preferred = s.release.protocol == dp.preferred_protocol;
        if delay == 0 {
            return Verdict::Accept;
        }
        if preferred && env.config().propers_and_repacks == ProperDownloadTypes::PreferAndUpgrade {
            for f in s.files {
                let same = s
                    .profile
                    .comparer()
                    .compare_quality(s.parsed.quality.quality, f.quality.quality)
                    == Ordering::Equal;
                if same && is_revision_upgrade(&f.quality, &s.parsed.quality) {
                    return Verdict::Accept;
                }
            }
        }
        if dp.bypass_if_highest_quality
            && preferred
            && let Some(best) = s.profile.last_allowed_quality()
            && s.profile
                .comparer()
                .compare_quality(s.parsed.quality.quality, best)
                != Ordering::Less
        {
            return Verdict::Accept;
        }
        if dp.bypass_if_above_custom_format_score
            && preferred
            && s.custom_format_score >= dp.minimum_custom_format_score
        {
            return Verdict::Accept;
        }
        let ids: Vec<i64> = s.episodes.iter().map(|e| e.id).collect();
        if let Some(oldest) = env.oldest_pending_age_minutes(s.series.id, &ids)
            && oldest > f64::from(delay)
        {
            return Verdict::Accept;
        }
        if s.release.age_minutes(env.now()) < f64::from(delay) {
            return Verdict::reject(R::MinimumAgeDelay, "Waiting for better quality release");
        }
        Verdict::Accept
    }
);

spec!(
    SeriesMatch,
    "Series",
    fn evaluate(&self, s: &EpisodeSubject<'_>, info: &TvDecisionInfo, _: Env) -> Verdict {
        match &info.search {
            Some(c) if c.series_id != s.series.id => {
                Verdict::reject(R::WrongSeries, "Wrong series")
            }
            _ => Verdict::Accept,
        }
    }
);

fn season_matches(s: &EpisodeSubject<'_>, season: i32) -> bool {
    if s.parsed.season_numbers.is_empty() {
        s.mapped_season == Some(season)
    } else {
        s.parsed.season_numbers.contains(&season)
    }
}

spec!(
    SeasonMatch,
    "SeasonMatch",
    fn evaluate(&self, s: &EpisodeSubject<'_>, info: &TvDecisionInfo, _: Env) -> Verdict {
        if let Some(c) = &info.search
            && let TvSearchKind::Season { season } = c.kind
            && !season_matches(s, season)
        {
            return Verdict::reject(R::WrongSeason, "Wrong season");
        }
        Verdict::Accept
    }
);

spec!(
    SingleEpisodeSearchMatch,
    "SingleEpisodeSearchMatch",
    fn evaluate(&self, s: &EpisodeSubject<'_>, info: &TvDecisionInfo, _: Env) -> Verdict {
        let Some(c) = &info.search else {
            return Verdict::Accept;
        };
        match c.kind {
            TvSearchKind::SingleEpisode { season, episode } => {
                if !season_matches(s, season) {
                    return Verdict::reject(R::WrongSeason, "Wrong season");
                }
                if s.parsed.episode_numbers.is_empty() {
                    return Verdict::reject(R::FullSeason, "Full season pack");
                }
                if !s.parsed.episode_numbers.contains(&episode) {
                    return Verdict::reject(R::WrongEpisode, "Wrong episode");
                }
                Verdict::Accept
            }
            TvSearchKind::AnimeEpisode { is_season_search }
                if s.parsed.full_season && !is_season_search =>
            {
                Verdict::reject(R::FullSeason, "Full season pack")
            }
            _ => Verdict::Accept,
        }
    }
);

spec!(
    EpisodeRequested,
    "EpisodeRequested",
    fn evaluate(&self, s: &EpisodeSubject<'_>, info: &TvDecisionInfo, _: Env) -> Verdict {
        let Some(c) = &info.search else {
            return Verdict::Accept;
        };
        if s.episodes.iter().any(|e| c.episode_ids.contains(&e.id)) {
            return Verdict::Accept;
        }
        let mut eps: Vec<_> = s.episodes.iter().collect();
        eps.sort_by_key(|e| (e.season_number, e.episode_number));
        let msg = match (eps.first(), eps.last()) {
            (Some(a), Some(b)) if eps.len() > 1 => format!(
                "Episode wasn't requested: {}x{}-{}",
                a.season_number, a.episode_number, b.episode_number
            ),
            (Some(a), _) => format!(
                "Episode wasn't requested: {}x{}",
                a.season_number, a.episode_number
            ),
            _ => "Episode wasn't requested".to_owned(),
        };
        Verdict::reject(R::WrongEpisode, msg)
    }
);

spec!(
    AirDate,
    "AirDate",
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        let profiles = env.release_profiles(&s.series.tags, s.release.indexer_id);
        let Some(best) = profiles
            .iter()
            .max_by_key(|p| (p.air_date_restriction, p.air_date_grace_period_hours))
        else {
            return Verdict::Accept;
        };
        if !best.air_date_restriction {
            return Verdict::Accept;
        }
        let grace = Duration::hours(i64::from(best.air_date_grace_period_hours));
        for e in s.episodes {
            let Some(aired) = e.air_date_utc else {
                return Verdict::reject(R::BeforeAirDate, "No air date available");
            };
            if s.release.publish_date < aired + grace {
                return Verdict::reject(
                    R::BeforeAirDate,
                    format!(
                        "Release date {} is before adjusted air date of {} (Air Date: {aired}. Grace period {} hours)",
                        s.release.publish_date,
                        aired + grace,
                        best.air_date_grace_period_hours
                    ),
                );
            }
        }
        Verdict::Accept
    }
);

spec!(
    SeasonPackOnly,
    "SeasonPackOnly",
    fn evaluate(&self, s: &EpisodeSubject<'_>, info: &TvDecisionInfo, env: Env) -> Verdict {
        let Some(c) = info.search.as_ref().filter(|c| c.episodes.len() > 1) else {
            return Verdict::Accept;
        };
        let days = env
            .indexer(s.release.indexer_id)
            .map_or(0, |i| i.season_pack_max_single_episode_age_days);
        if days == 0 || s.series.series_type != SeriesType::Standard || s.parsed.full_season {
            return Verdict::Accept;
        }
        let Some(first) = s.episodes.first() else {
            return Verdict::Accept;
        };
        let latest = c
            .episodes
            .iter()
            .filter(|(season, _)| *season == first.season_number)
            .filter_map(|(_, aired)| *aired)
            .max();
        if latest.is_some_and(|l| l < env.now() - Duration::days(i64::from(days))) {
            return Verdict::reject(
                R::NotSeasonPack,
                format!(
                    "Last episode in this season aired more than {days} days ago, season pack required."
                ),
            );
        }
        Verdict::Accept
    }
);

spec!(
    FreeSpace,
    "FreeSpace",
    fn priority(&self) -> Priority {
        Priority::Disk
    },
    fn evaluate(&self, s: &EpisodeSubject<'_>, _: &TvDecisionInfo, env: Env) -> Verdict {
        let config = env.config();
        if config.skip_free_space_check_when_importing {
            return Verdict::Accept;
        }
        let Some(free) = env.free_space(&s.series.path) else {
            return Verdict::Accept;
        };
        let minimum = megabytes(config.minimum_free_space_when_importing as f64);
        let remaining = free as i64 - s.release.size as i64;
        if remaining <= 0 {
            return Verdict::reject(
                R::MinimumFreeSpace,
                "Importing after download will exceed available disk space",
            );
        }
        if remaining < minimum {
            return Verdict::reject(
                R::MinimumFreeSpace,
                format!(
                    "Not enough free space ({}) to import after download: {}. (Settings: Media Management: Minimum Free Space)",
                    size_suffix(minimum),
                    size_suffix(remaining)
                ),
            );
        }
        Verdict::Accept
    }
);

pub fn default_tv_specs() -> Vec<Box<dyn TvSpecification>> {
    vec![
        Box::new(MinimumAge),
        Box::new(Retention),
        Box::new(AcceptableSize),
        Box::new(MaximumSize),
        Box::new(NotSample),
        Box::new(RawDisk),
        Box::new(Protocol),
        Box::new(QualityAllowedByProfile),
        Box::new(CustomFormatAllowedByProfile),
        Box::new(ReleaseRestrictions),
        Box::new(TorrentSeeding),
        Box::new(IndexerTag),
        Box::new(MonitoredEpisode),
        Box::new(MultiSeason),
        Box::new(SplitEpisode),
        Box::new(FullSeason),
        Box::new(SameEpisodesGrab),
        Box::new(SeriesMatch),
        Box::new(SeasonMatch),
        Box::new(SeasonPackOnly),
        Box::new(AirDate),
        Box::new(SingleEpisodeSearchMatch),
        Box::new(EpisodeRequested),
        Box::new(Proper),
        Box::new(UpgradeAllowed),
        Box::new(UpgradeDisk),
        Box::new(AnimeVersionUpgrade),
        Box::new(Queue),
        Box::new(Blocklist),
        Box::new(BlockedIndexer),
        Box::new(AlreadyImported),
        Box::new(Repack),
        Box::new(History),
        Box::new(Pending),
        Box::new(Delay),
        Box::new(FreeSpace),
    ]
}
