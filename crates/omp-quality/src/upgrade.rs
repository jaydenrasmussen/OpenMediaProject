//! Is a new release an upgrade over what we have? Ported from Radarr
//! `UpgradableSpecification` (GPL-3.0). The same rules run against the file on
//! disk, recent history, and the download queue.

use std::cmp::Ordering;

use omp_core::quality::QualityModel;
use serde::{Deserialize, Serialize};

use crate::profile::QualityProfile;

/// Why a release is (not) an upgrade. `None` means it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeRejectReason {
    None,
    BetterQuality,
    BetterRevision,
    QualityCutoff,
    CustomFormatScore,
    CustomFormatCutoff,
    MinCustomFormatScore,
    UpgradesNotAllowed,
}

/// Media-management setting for propers and repacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProperDownloadTypes {
    #[default]
    PreferAndUpgrade,
    DoNotUpgrade,
    DoNotPrefer,
}

/// A release with the custom formats it matched (by format id).
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub quality: &'a QualityModel,
    pub format_ids: &'a [i32],
}

/// Same quality (not just the same rank) and a higher revision.
pub fn is_revision_upgrade(current: &QualityModel, new: &QualityModel) -> bool {
    current.quality == new.quality
        && new.revision.partial_cmp(&current.revision) == Some(Ordering::Greater)
}

pub fn quality_cutoff_not_met(
    profile: &QualityProfile,
    current: &QualityModel,
    new: Option<&QualityModel>,
) -> bool {
    let cutoff = if profile.upgrade_allowed {
        profile.cutoff
    } else {
        profile.first_allowed_quality().map_or(0, |q| q.id)
    };
    if profile
        .comparer()
        .compare_ids(current.quality.id, cutoff, false)
        == Ordering::Less
    {
        return true;
    }
    new.is_some_and(|n| is_revision_upgrade(current, n))
}

fn custom_format_cutoff_not_met(profile: &QualityProfile, current_formats: &[i32]) -> bool {
    let score = profile.calculate_custom_format_score(current_formats);
    let cutoff = if profile.upgrade_allowed {
        profile.cutoff_format_score
    } else {
        profile.min_format_score
    };
    score < cutoff
}

/// True while the existing file is below the wanted quality or format score.
pub fn cutoff_not_met(
    profile: &QualityProfile,
    current: &QualityModel,
    current_formats: &[i32],
    new: Option<&QualityModel>,
) -> bool {
    quality_cutoff_not_met(profile, current, new)
        || custom_format_cutoff_not_met(profile, current_formats)
}

pub fn is_upgradable(
    profile: &QualityProfile,
    propers: ProperDownloadTypes,
    current: Candidate<'_>,
    new: Candidate<'_>,
) -> UpgradeRejectReason {
    use UpgradeRejectReason as R;

    let comparer = profile.comparer();
    let quality_compare = comparer.compare_quality(new.quality.quality, current.quality.quality);

    if quality_compare == Ordering::Greater
        && quality_cutoff_not_met(profile, current.quality, Some(new.quality))
    {
        return R::None;
    }
    if quality_compare == Ordering::Less {
        return R::BetterQuality;
    }

    let revision_compare = new
        .quality
        .revision
        .partial_cmp(&current.quality.revision)
        .unwrap_or(Ordering::Equal);

    // Propers are accepted unless the user opted out of preferring them;
    // custom-format words can still prefer propers in that case.
    if propers != ProperDownloadTypes::DoNotPrefer && revision_compare == Ordering::Greater {
        return R::None;
    }
    if !profile.upgrade_allowed {
        return R::UpgradesNotAllowed;
    }
    if propers != ProperDownloadTypes::DoNotPrefer && revision_compare == Ordering::Less {
        return R::BetterRevision;
    }
    if quality_compare == Ordering::Greater {
        return R::QualityCutoff;
    }

    let current_score = profile.calculate_custom_format_score(current.format_ids);
    let new_score = profile.calculate_custom_format_score(new.format_ids);
    if new_score <= current_score {
        return R::CustomFormatScore;
    }
    if current_score >= profile.cutoff_format_score {
        return R::CustomFormatCutoff;
    }
    if new_score < current_score + profile.min_upgrade_format_score {
        return R::MinCustomFormatScore;
    }
    R::None
}

/// Whether the profile lets us replace the current file with the new release.
pub fn is_upgrade_allowed(
    profile: &QualityProfile,
    current: Candidate<'_>,
    new: Candidate<'_>,
) -> bool {
    let quality_upgrade = profile
        .comparer()
        .compare_model(new.quality, current.quality)
        == Ordering::Greater;
    let format_upgrade = profile.calculate_custom_format_score(new.format_ids)
        > profile.calculate_custom_format_score(current.format_ids);

    if is_revision_upgrade(current.quality, new.quality) {
        return true;
    }
    if quality_upgrade || format_upgrade {
        return profile.upgrade_allowed;
    }
    true
}

#[cfg(test)]
mod tests {
    use omp_core::quality::{Quality, QualityModel, Revision};

    use super::*;
    use crate::profile::{ProfileFormatItem, test_support::profile};

    fn model(quality: Quality) -> QualityModel {
        QualityModel {
            quality,
            ..Default::default()
        }
    }

    fn revised(quality: Quality, version: u32) -> QualityModel {
        QualityModel {
            quality,
            revision: Revision {
                version,
                real: 0,
                is_repack: false,
            },
            ..Default::default()
        }
    }

    fn cand<'a>(q: &'a QualityModel, f: &'a [i32]) -> Candidate<'a> {
        Candidate {
            quality: q,
            format_ids: f,
        }
    }

    fn check(
        p: &QualityProfile,
        propers: ProperDownloadTypes,
        current: &QualityModel,
        new: &QualityModel,
    ) -> UpgradeRejectReason {
        is_upgradable(p, propers, cand(current, &[]), cand(new, &[]))
    }

    use ProperDownloadTypes::*;
    use UpgradeRejectReason as R;

    #[test]
    fn higher_quality_below_cutoff_is_an_upgrade() {
        // cutoff is the web group (row 2); DVD -> Bluray-1080p is above it.
        let p = profile();
        assert_eq!(
            check(
                &p,
                PreferAndUpgrade,
                &model(Quality::DVD),
                &model(Quality::WEBDL_720P)
            ),
            R::None
        );
        assert_eq!(
            check(
                &p,
                PreferAndUpgrade,
                &model(Quality::DVD),
                &model(Quality::BLURAY_1080P)
            ),
            R::None
        );
    }

    #[test]
    fn lower_quality_is_rejected() {
        let p = profile();
        assert_eq!(
            check(
                &p,
                PreferAndUpgrade,
                &model(Quality::BLURAY_1080P),
                &model(Quality::DVD)
            ),
            R::BetterQuality
        );
    }

    #[test]
    fn higher_quality_when_cutoff_is_met_is_rejected_as_cutoff() {
        let p = profile();
        // Existing WEB 1080p meets the (group) cutoff; Bluray is better but not wanted.
        assert_eq!(
            check(
                &p,
                PreferAndUpgrade,
                &model(Quality::WEBDL_1080P),
                &model(Quality::BLURAY_1080P)
            ),
            R::QualityCutoff
        );
    }

    #[test]
    fn proper_over_same_quality() {
        let p = profile();
        let cur = model(Quality::WEBDL_1080P);
        let proper = revised(Quality::WEBDL_1080P, 2);
        assert_eq!(check(&p, PreferAndUpgrade, &cur, &proper), R::None);
        assert_eq!(check(&p, DoNotPrefer, &cur, &proper), R::CustomFormatScore);

        let mut no_upgrades = p.clone();
        no_upgrades.upgrade_allowed = false;
        assert_eq!(
            check(&no_upgrades, DoNotPrefer, &cur, &proper),
            R::UpgradesNotAllowed
        );
        // Proper accepted even when upgrades are off, if propers are preferred.
        assert_eq!(
            check(&no_upgrades, PreferAndUpgrade, &cur, &proper),
            R::None
        );
    }

    #[test]
    fn older_revision_is_rejected() {
        let p = profile();
        assert_eq!(
            check(
                &p,
                PreferAndUpgrade,
                &revised(Quality::WEBDL_1080P, 2),
                &model(Quality::WEBDL_1080P)
            ),
            R::BetterRevision
        );
    }

    #[test]
    fn upgrades_off_blocks_quality_upgrades_but_not_propers() {
        let mut p = profile();
        p.upgrade_allowed = false;
        // The cutoff becomes the first allowed quality (DVD), so DVD is never
        // "below cutoff" and a better quality is not taken.
        assert_eq!(
            check(
                &p,
                PreferAndUpgrade,
                &model(Quality::DVD),
                &model(Quality::WEBDL_720P)
            ),
            R::UpgradesNotAllowed
        );
        // A proper of the same quality still goes through.
        assert_eq!(
            check(
                &p,
                PreferAndUpgrade,
                &model(Quality::DVD),
                &revised(Quality::DVD, 2)
            ),
            R::None
        );
    }

    fn scored_profile() -> QualityProfile {
        let mut p = profile();
        p.format_items = vec![
            ProfileFormatItem {
                format_id: 1,
                score: 50,
            },
            ProfileFormatItem {
                format_id: 2,
                score: 10,
            },
        ];
        p.cutoff_format_score = 100;
        p.min_upgrade_format_score = 5;
        p
    }

    #[test]
    fn custom_format_rules_for_same_quality() {
        let p = scored_profile();
        let q = model(Quality::WEBDL_1080P);
        let up = |cur: &[i32], new: &[i32]| {
            is_upgradable(&p, PreferAndUpgrade, cand(&q, cur), cand(&q, new))
        };

        assert_eq!(up(&[], &[]), R::CustomFormatScore, "no improvement");
        assert_eq!(up(&[1], &[2]), R::CustomFormatScore, "lower score");
        assert_eq!(up(&[], &[1]), R::None, "50 > 0 and under the cutoff");
        assert_eq!(
            up(&[1, 2], &[1, 2]),
            R::CustomFormatScore,
            "equal is not better"
        );

        let mut high = p.clone();
        high.cutoff_format_score = 40;
        assert_eq!(
            is_upgradable(&high, PreferAndUpgrade, cand(&q, &[1]), cand(&q, &[1, 2])),
            R::CustomFormatCutoff,
            "existing score 50 already meets the cutoff of 40"
        );

        let mut step = p.clone();
        step.min_upgrade_format_score = 20;
        assert_eq!(
            is_upgradable(&step, PreferAndUpgrade, cand(&q, &[]), cand(&q, &[2])),
            R::MinCustomFormatScore,
            "gain of 10 < required 20"
        );
    }

    #[test]
    fn cutoff_not_met_considers_quality_and_format_score() {
        let p = scored_profile();
        assert!(
            cutoff_not_met(&p, &model(Quality::DVD), &[1, 2], None),
            "quality below cutoff"
        );
        assert!(
            cutoff_not_met(&p, &model(Quality::WEBDL_1080P), &[1], None),
            "score 50 < 100"
        );
        let mut met = p.clone();
        met.cutoff_format_score = 50;
        assert!(!cutoff_not_met(
            &met,
            &model(Quality::WEBDL_1080P),
            &[1],
            None
        ));
        // A proper keeps the search open even when both cutoffs are met.
        let proper = revised(Quality::WEBDL_1080P, 2);
        assert!(cutoff_not_met(
            &met,
            &model(Quality::WEBDL_1080P),
            &[1],
            Some(&proper)
        ));
    }

    #[test]
    fn cutoff_without_upgrades_uses_first_allowed_and_min_score() {
        let mut p = scored_profile();
        p.upgrade_allowed = false;
        p.min_format_score = 10;
        // first allowed is DVD, so nothing allowed is below the cutoff.
        assert!(!cutoff_not_met(&p, &model(Quality::DVD), &[1], None));
        assert!(
            cutoff_not_met(&p, &model(Quality::DVD), &[], None),
            "score 0 < min 10"
        );
    }

    #[test]
    fn upgrade_allowed_rules() {
        let p = profile();
        let dvd = model(Quality::DVD);
        let bluray = model(Quality::BLURAY_1080P);
        assert!(is_upgrade_allowed(&p, cand(&dvd, &[]), cand(&bluray, &[])));

        let mut off = p.clone();
        off.upgrade_allowed = false;
        assert!(!is_upgrade_allowed(
            &off,
            cand(&dvd, &[]),
            cand(&bluray, &[])
        ));
        // Downgrades and sidegrades are "allowed" here; other rules reject them.
        assert!(is_upgrade_allowed(
            &off,
            cand(&bluray, &[]),
            cand(&dvd, &[])
        ));
        // Revision upgrades are always allowed.
        let proper = revised(Quality::DVD, 2);
        assert!(is_upgrade_allowed(
            &off,
            cand(&dvd, &[]),
            cand(&proper, &[])
        ));
    }

    #[test]
    fn revision_upgrade_needs_the_same_quality() {
        let a = revised(Quality::WEBDL_1080P, 1);
        let b = revised(Quality::WEBDL_1080P, 2);
        let c = revised(Quality::WEBRIP_1080P, 2);
        assert!(is_revision_upgrade(&a, &b));
        assert!(
            !is_revision_upgrade(&a, &c),
            "WEBRip proper is not an upgrade of a WEB-DL"
        );
        assert!(!is_revision_upgrade(&b, &a));
    }
}
