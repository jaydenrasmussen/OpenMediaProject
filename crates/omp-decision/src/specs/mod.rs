//! The specifications. Each answers one question about a release; the engine
//! runs them in priority groups (see `engine`).

mod disk;
mod release;
mod upgrade;

use omp_core::quality::QualityModel;
use omp_quality::{
    profile::{ProfileItem, QualityProfile},
    upgrade::UpgradeRejectReason,
};

use crate::{
    model::{DecisionData, DecisionInfo, Subject},
    reason::{Family, RejectionKind, Verdict},
};

pub use disk::FreeSpace;
pub use release::{
    AcceptableSize, Availability, BlockedIndexer, Blocklist, HardcodeSubs, IndexerTag, Language,
    MaximumSize, MinimumAge, MonitoredMovie, NotSample, Protocol, RawDisk, ReleaseRestrictions,
    RequiredIndexerFlags, Retention, SearchMovie, TorrentSeeding,
};
pub use upgrade::{
    AlreadyImported, CustomFormatAllowedByProfile, Delay, History, Pending, Proper,
    QualityAllowedByProfile, Queue, Repack, UpgradeAllowed, UpgradeDisk,
};

/// Specs in a lower group run first; the first group with a rejection ends
/// the run. Radarr also has "Parsing" and "Database", which equal `Default`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Default = 0,
    /// Checks that touch the disk, so they run last.
    Disk = 1,
}

pub trait Specification: Send + Sync {
    fn name(&self) -> &'static str;
    fn priority(&self) -> Priority {
        Priority::Default
    }
    fn kind(&self) -> RejectionKind {
        RejectionKind::Permanent
    }
    fn evaluate(
        &self,
        subject: &Subject<'_>,
        info: &DecisionInfo,
        env: &dyn DecisionData,
    ) -> Verdict;
}

/// Every movie spec, in Radarr's order.
pub fn default_specs() -> Vec<Box<dyn Specification>> {
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
        Box::new(Language),
        Box::new(HardcodeSubs),
        Box::new(ReleaseRestrictions),
        Box::new(RequiredIndexerFlags),
        Box::new(TorrentSeeding),
        Box::new(IndexerTag),
        Box::new(MonitoredMovie),
        Box::new(Availability),
        Box::new(SearchMovie),
        Box::new(Proper),
        Box::new(UpgradeAllowed),
        Box::new(UpgradeDisk),
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

// ---- shared helpers ----------------------------------------------------------

/// Whole bytes for a size in megabytes (`Convert.ToInt64(mb * 1024 * 1024)`).
pub(crate) fn megabytes(mb: f64) -> i64 {
    (mb * 1024.0 * 1024.0).round_ties_even() as i64
}

/// "1.5 GB" style size for messages.
pub(crate) fn size_suffix(bytes: i64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value.abs() >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Display name of the profile row holding `id`, like "WEB 1080p (WEBDL-1080p)".
pub(crate) fn profile_item_name(profile: &QualityProfile, id: u32) -> String {
    let index = profile.index_of_id(id, false).index;
    match profile.items.get(index) {
        Some(ProfileItem::Quality { quality, .. }) => quality.name.to_owned(),
        Some(ProfileItem::Group {
            name, qualities, ..
        }) => {
            let list: Vec<_> = qualities.iter().map(|q| q.name).collect();
            if name.trim().is_empty() {
                list.join(", ")
            } else {
                format!("{name} ({})", list.join(", "))
            }
        }
        None => String::new(),
    }
}

/// Turns an upgrade check against an existing item (history, queue or file)
/// into a rejection. `current` is that item's quality and custom formats.
pub(crate) fn upgrade_verdict(
    family: Family,
    reason: UpgradeRejectReason,
    profile: &QualityProfile,
    current: &QualityModel,
    current_format_ids: &[i32],
    prefix: &str,
) -> Verdict {
    let label = family.label();
    let head = if prefix.is_empty() {
        let mut chars = label.chars();
        chars
            .next()
            .map(|c| c.to_uppercase().chain(chars).collect::<String>())
            .unwrap_or_default()
    } else {
        format!("{prefix} {label}")
    };
    let sentence = |tail: String| format!("{head} {tail}");
    match reason {
        UpgradeRejectReason::None => Verdict::Accept,
        UpgradeRejectReason::BetterQuality => Verdict::reject(
            family.higher_preference(),
            sentence(format!(
                "is of equal or higher preference: {}",
                current.quality
            )),
        ),
        UpgradeRejectReason::BetterRevision => Verdict::reject(
            family.higher_revision(),
            sentence(format!(
                "is of equal or higher revision: v{}",
                current.revision.version
            )),
        ),
        UpgradeRejectReason::QualityCutoff => Verdict::reject(
            family.cutoff_met(),
            sentence(format!(
                "meets quality cutoff: {}",
                profile_item_name(profile, profile.cutoff)
            )),
        ),
        UpgradeRejectReason::CustomFormatCutoff => Verdict::reject(
            family.custom_format_cutoff_met(),
            sentence(format!(
                "meets Custom Format cutoff: {}",
                profile.cutoff_format_score
            )),
        ),
        UpgradeRejectReason::CustomFormatScore => Verdict::reject(
            family.custom_format_score(),
            sentence(format!(
                "has an equal or higher Custom Format score: {}",
                profile.calculate_custom_format_score(current_format_ids)
            )),
        ),
        UpgradeRejectReason::MinCustomFormatScore => Verdict::reject(
            family.custom_format_increment(),
            sentence(format!(
                "has Custom Format score within Custom Format score increment: {}",
                profile.min_upgrade_format_score
            )),
        ),
        UpgradeRejectReason::UpgradesNotAllowed => Verdict::reject(
            family.upgrades_not_allowed(),
            sentence(format!(
                "and Quality Profile '{}' does not allow upgrades",
                profile.name
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_formatting() {
        assert_eq!(size_suffix(0), "0.0 B");
        assert_eq!(size_suffix(1536), "1.5 KB");
        assert_eq!(size_suffix(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn megabytes_rounds_like_dotnet() {
        assert_eq!(megabytes(1.0), 1_048_576);
        assert_eq!(megabytes(0.0), 0);
        assert_eq!(megabytes(95.0), 99_614_720);
    }

    #[test]
    fn spec_order_and_priorities() {
        let specs = default_specs();
        assert_eq!(specs.len(), 30);
        let disk: Vec<_> = specs
            .iter()
            .filter(|s| s.priority() == Priority::Disk)
            .map(|s| s.name())
            .collect();
        assert_eq!(disk, ["FreeSpace"]);
        let mut names: Vec<_> = specs.iter().map(|s| s.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 30, "spec names are unique");
    }
}
