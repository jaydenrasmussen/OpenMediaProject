//! Checks that compare a release with what we already have or are fetching.

use chrono::Duration;
use omp_quality::upgrade::{
    Candidate, ProperDownloadTypes, UpgradeRejectReason, cutoff_not_met, is_revision_upgrade,
    is_upgradable, is_upgrade_allowed,
};

use super::{Specification, profile_item_name, upgrade_verdict};
use crate::{
    model::{DecisionData, DecisionInfo, HistoryEvent, ReleaseSource, Subject},
    reason::{Family, RejectionKind, RejectionReason as R, Verdict},
};

pub struct QualityAllowedByProfile;

impl Specification for QualityAllowedByProfile {
    fn name(&self) -> &'static str {
        "QualityAllowedByProfile"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, _: &dyn DecisionData) -> Verdict {
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
}

pub struct CustomFormatAllowedByProfile;

impl Specification for CustomFormatAllowedByProfile {
    fn name(&self) -> &'static str {
        "CustomFormatAllowedByProfile"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, _: &dyn DecisionData) -> Verdict {
        let minimum = s.profile.min_format_score;
        if s.custom_format_score < minimum {
            return Verdict::reject(
                R::CustomFormatMinimumScore,
                format!(
                    "Custom Formats have score {} below Movie's profile minimum {minimum}",
                    s.custom_format_score
                ),
            );
        }
        Verdict::Accept
    }
}

fn new_candidate<'a>(s: &'a Subject<'_>) -> Candidate<'a> {
    Candidate {
        quality: &s.parsed.quality,
        format_ids: s.custom_format_ids,
    }
}

pub struct UpgradeAllowed;

impl Specification for UpgradeAllowed {
    fn name(&self) -> &'static str {
        "UpgradeAllowed"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, _: &dyn DecisionData) -> Verdict {
        let Some(file) = s.existing_file else {
            return Verdict::Accept;
        };
        let current = Candidate {
            quality: &file.quality,
            format_ids: &file.custom_format_ids,
        };
        if !is_upgrade_allowed(s.profile, current, new_candidate(s)) {
            return Verdict::reject(
                R::QualityUpgradesDisabled,
                "Existing file and the Quality profile does not allow upgrades",
            );
        }
        Verdict::Accept
    }
}

pub struct UpgradeDisk;

impl Specification for UpgradeDisk {
    fn name(&self) -> &'static str {
        "UpgradeDisk"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        let Some(file) = s.existing_file else {
            return Verdict::Accept;
        };
        let profile = s.profile;
        if !cutoff_not_met(
            profile,
            &file.quality,
            &file.custom_format_ids,
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
            Candidate {
                quality: &file.quality,
                format_ids: &file.custom_format_ids,
            },
            new_candidate(s),
        );
        upgrade_verdict(
            Family::Disk,
            reason,
            profile,
            &file.quality,
            &file.custom_format_ids,
            "",
        )
    }
}

pub struct Queue;

impl Specification for Queue {
    fn name(&self) -> &'static str {
        "Queue"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        let propers = env.config().propers_and_repacks;
        for item in env.queue_for_movie(s.movie.id) {
            if item.failed_pending {
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
                        "Quality for release in queue already meets cutoff: {}",
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
}

pub struct History;

impl Specification for History {
    fn name(&self) -> &'static str {
        "History"
    }

    fn evaluate(&self, s: &Subject<'_>, info: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        if info.search.is_some() {
            return Verdict::Accept;
        }
        let cdh = env.config().enable_completed_download_handling;
        let history = env.history_for_movie(s.movie.id);
        let Some(recent) = history.first().filter(|h| h.event == HistoryEvent::Grabbed) else {
            return Verdict::Accept;
        };

        let is_recent = recent.date > env.now() - Duration::hours(12);
        if !is_recent && cdh {
            return Verdict::Accept;
        }

        let cutoff_unmet = cutoff_not_met(
            s.profile,
            &recent.quality,
            &recent.custom_format_ids,
            Some(&s.parsed.quality),
        );
        if !cutoff_unmet {
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
        upgrade_verdict(
            Family::History,
            reason,
            s.profile,
            &recent.quality,
            &recent.custom_format_ids,
            prefix,
        )
    }
}

pub struct AlreadyImported;

impl Specification for AlreadyImported {
    fn name(&self) -> &'static str {
        "AlreadyImported"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        if !env.config().enable_completed_download_handling || s.existing_file.is_none() {
            return Verdict::Accept;
        }
        let history = env.history_for_movie(s.movie.id);
        let Some(grabbed) = history.iter().find(|h| h.event == HistoryEvent::Grabbed) else {
            return Verdict::Accept;
        };
        let imported = history.iter().find(|h| {
            h.event == HistoryEvent::DownloadFolderImported && h.download_id == grabbed.download_id
        });
        let Some(imported) = imported else {
            return Verdict::Accept;
        };
        if grabbed.quality == imported.quality {
            return Verdict::Accept;
        }

        if let (Some(torrent), Some(download_id)) = (&s.release.torrent, &grabbed.download_id)
            && torrent
                .info_hash
                .as_deref()
                .is_some_and(|h| h.to_uppercase() == *download_id)
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
        Verdict::Accept
    }
}

pub struct Proper;

impl Specification for Proper {
    fn name(&self) -> &'static str {
        "Proper"
    }

    fn evaluate(&self, s: &Subject<'_>, info: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        if info.search.is_some() {
            return Verdict::Accept;
        }
        let propers = env.config().propers_and_repacks;
        if propers == ProperDownloadTypes::DoNotPrefer {
            return Verdict::Accept;
        }
        let Some(file) = s.existing_file else {
            return Verdict::Accept;
        };
        if is_revision_upgrade(&file.quality, &s.parsed.quality) {
            if propers == ProperDownloadTypes::DoNotUpgrade {
                return Verdict::reject(R::PropersDisabled, "Proper downloading is disabled");
            }
            let today = env
                .now()
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .map(|d| d.and_utc())
                .unwrap_or_else(|| env.now());
            if file.date_added < today - Duration::days(7) {
                return Verdict::reject(R::ProperForOldFile, "Proper for old file");
            }
        }
        Verdict::Accept
    }
}

pub struct Repack;

impl Specification for Repack {
    fn name(&self) -> &'static str {
        "Repack"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        let propers = env.config().propers_and_repacks;
        if !s.parsed.quality.revision.is_repack || propers == ProperDownloadTypes::DoNotPrefer {
            return Verdict::Accept;
        }
        let Some(file) = s.existing_file else {
            return Verdict::Accept;
        };
        if !is_revision_upgrade(&file.quality, &s.parsed.quality) {
            return Verdict::Accept;
        }
        if propers == ProperDownloadTypes::DoNotUpgrade {
            return Verdict::reject(R::RepackDisabled, "Repack downloading is disabled");
        }
        let blank = |g: &Option<String>| g.as_deref().is_none_or(|g| g.trim().is_empty());
        if blank(&file.release_group) {
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
        let (release, existing) = (
            s.parsed.release_group.as_deref().unwrap_or_default(),
            file.release_group.as_deref().unwrap_or_default(),
        );
        if release.to_lowercase() != existing.to_lowercase() {
            return Verdict::reject(
                R::RepackReleaseGroupDoesNotMatch,
                format!(
                    "Release is a repack for a different release group. Release Group: {release}. File release group: {existing}"
                ),
            );
        }
        Verdict::Accept
    }
}

pub struct Pending;

impl Specification for Pending {
    fn name(&self) -> &'static str {
        "Pending"
    }

    fn kind(&self) -> RejectionKind {
        RejectionKind::Temporary
    }

    fn evaluate(&self, s: &Subject<'_>, info: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        if s.source == ReleaseSource::Rss || info.user_invoked_search() {
            return Verdict::Accept;
        }
        if env.movie_has_pending_release(s.movie.id) {
            return Verdict::reject(
                R::MinimumAgeDelayPushed,
                "Release containing at least one matching movie is already pending, delaying pushed release",
            );
        }
        Verdict::Accept
    }
}

pub struct Delay;

impl Specification for Delay {
    fn name(&self) -> &'static str {
        "Delay"
    }

    fn kind(&self) -> RejectionKind {
        RejectionKind::Temporary
    }

    fn evaluate(&self, s: &Subject<'_>, info: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        if info.user_invoked_search() {
            return Verdict::Accept;
        }
        let delay_profile = env.delay_profile_for(&s.movie.tags);
        let delay = delay_profile.protocol_delay(s.release.protocol);
        let preferred = s.release.protocol == delay_profile.preferred_protocol;
        if delay == 0 {
            return Verdict::Accept;
        }

        // A proper that really upgrades the file is never worth waiting on.
        if preferred && let Some(file) = s.existing_file {
            let reason = is_upgradable(
                s.profile,
                env.config().propers_and_repacks,
                Candidate {
                    quality: &file.quality,
                    format_ids: &file.custom_format_ids,
                },
                new_candidate(s),
            );
            if reason == UpgradeRejectReason::None
                && is_revision_upgrade(&file.quality, &s.parsed.quality)
            {
                return Verdict::Accept;
            }
        }

        if delay_profile.bypass_if_highest_quality
            && preferred
            && let Some(best) = s.profile.last_allowed_quality()
            && s.profile
                .comparer()
                .compare_quality(s.parsed.quality.quality, best)
                != std::cmp::Ordering::Less
        {
            return Verdict::Accept;
        }

        if delay_profile.bypass_if_above_custom_format_score
            && preferred
            && s.custom_format_score >= delay_profile.minimum_custom_format_score
        {
            return Verdict::Accept;
        }

        if let Some(oldest) = env.oldest_pending_release_age_minutes(s.movie.id)
            && oldest > f64::from(delay)
        {
            return Verdict::Accept;
        }

        if s.release.age_minutes(env.now()) < f64::from(delay) {
            return Verdict::reject(R::MinimumAgeDelay, "Waiting for better quality release");
        }
        Verdict::Accept
    }
}
