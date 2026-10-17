//! Quality profiles: an ordered list of qualities (or groups of qualities),
//! which of them are allowed, the cutoff, and custom-format scoring.

use std::cmp::Ordering;

use omp_core::{
    language::Language,
    quality::{Quality, QualityModel},
};

/// One row of a profile: a single quality, or a named group that ranks as one.
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileItem {
    Quality {
        quality: Quality,
        allowed: bool,
    },
    Group {
        /// Group ids are > 0 and share the id space with quality ids when
        /// used as a cutoff, so they start above the highest quality id.
        id: u32,
        name: String,
        qualities: Vec<Quality>,
        allowed: bool,
    },
}

impl ProfileItem {
    pub fn allowed(&self) -> bool {
        match self {
            ProfileItem::Quality { allowed, .. } | ProfileItem::Group { allowed, .. } => *allowed,
        }
    }

    pub fn qualities(&self) -> Vec<Quality> {
        match self {
            ProfileItem::Quality { quality, .. } => vec![*quality],
            ProfileItem::Group { qualities, .. } => qualities.clone(),
        }
    }
}

/// A custom format and the score this profile gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileFormatItem {
    pub format_id: i32,
    pub score: i32,
}

/// Position of a quality in a profile. Compare with [`QualityIndex::compare`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QualityIndex {
    pub index: usize,
    pub group_index: usize,
}

impl QualityIndex {
    /// With `respect_group_order`, qualities inside one group are ordered too.
    pub fn compare(self, other: QualityIndex, respect_group_order: bool) -> Ordering {
        let by_index = self.index.cmp(&other.index);
        if respect_group_order && by_index == Ordering::Equal {
            self.group_index.cmp(&other.group_index)
        } else {
            by_index
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct QualityProfile {
    pub id: i32,
    pub name: String,
    /// Quality id, or group id, below which upgrades are still wanted.
    pub cutoff: u32,
    /// Lowest to highest.
    pub items: Vec<ProfileItem>,
    pub min_format_score: i32,
    pub cutoff_format_score: i32,
    pub min_upgrade_format_score: i32,
    pub format_items: Vec<ProfileFormatItem>,
    pub language: Language,
    pub upgrade_allowed: bool,
}

impl QualityProfile {
    /// First allowed quality in profile order.
    pub fn first_allowed_quality(&self) -> Option<Quality> {
        let item = self.items.iter().find(|i| i.allowed())?;
        item.qualities().first().copied()
    }

    /// Last allowed quality in profile order.
    pub fn last_allowed_quality(&self) -> Option<Quality> {
        let item = self.items.iter().rev().find(|i| i.allowed())?;
        item.qualities().last().copied()
    }

    /// Where `id` sits in the profile. An id the profile does not mention maps
    /// to index 0, like Radarr: unknown qualities rank lowest.
    pub fn index_of_id(&self, id: u32, respect_group_order: bool) -> QualityIndex {
        for (i, item) in self.items.iter().enumerate() {
            match item {
                ProfileItem::Quality { quality, .. } if quality.id == id => {
                    return QualityIndex {
                        index: i,
                        group_index: 0,
                    };
                }
                ProfileItem::Group {
                    id: group_id,
                    qualities,
                    ..
                } => {
                    if *group_id > 0 && *group_id == id {
                        return QualityIndex {
                            index: i,
                            group_index: 0,
                        };
                    }
                    if let Some(g) = qualities.iter().position(|q| q.id == id) {
                        return QualityIndex {
                            index: i,
                            group_index: if respect_group_order { g } else { 0 },
                        };
                    }
                }
                _ => {}
            }
        }
        QualityIndex::default()
    }

    pub fn index_of(&self, quality: Quality, respect_group_order: bool) -> QualityIndex {
        self.index_of_id(quality.id, respect_group_order)
    }

    /// Sum of the scores of the profile's formats that are in `matched`.
    pub fn calculate_custom_format_score(&self, matched_format_ids: &[i32]) -> i32 {
        self.format_items
            .iter()
            .filter(|f| matched_format_ids.contains(&f.format_id))
            .map(|f| f.score)
            .sum()
    }

    pub fn comparer(&self) -> QualityModelComparer<'_> {
        QualityModelComparer { profile: self }
    }
}

/// Orders qualities by their position in one profile (higher is better).
pub struct QualityModelComparer<'p> {
    profile: &'p QualityProfile,
}

impl QualityModelComparer<'_> {
    pub fn compare_ids(&self, left: u32, right: u32, respect_group_order: bool) -> Ordering {
        self.profile.index_of_id(left, respect_group_order).compare(
            self.profile.index_of_id(right, respect_group_order),
            respect_group_order,
        )
    }

    pub fn compare_quality(&self, left: Quality, right: Quality) -> Ordering {
        self.compare_ids(left.id, right.id, false)
    }

    /// Quality position first, then revision.
    pub fn compare_model(&self, left: &QualityModel, right: &QualityModel) -> Ordering {
        self.compare_quality(left.quality, right.quality)
            .then_with(|| {
                left.revision
                    .partial_cmp(&right.revision)
                    .unwrap_or(Ordering::Equal)
            })
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    fn q(quality: Quality, allowed: bool) -> ProfileItem {
        ProfileItem::Quality { quality, allowed }
    }

    /// SDTV, DVD, WEB 720p/1080p as a group, Bluray-1080p; cutoff Bluray-1080p's
    /// neighbour: the web group.
    pub fn profile() -> QualityProfile {
        QualityProfile {
            id: 1,
            name: "test".into(),
            cutoff: 1001,
            items: vec![
                q(Quality::SDTV, false),
                q(Quality::DVD, true),
                ProfileItem::Group {
                    id: 1001,
                    name: "WEB 720p/1080p".into(),
                    qualities: vec![Quality::WEBDL_720P, Quality::WEBDL_1080P],
                    allowed: true,
                },
                q(Quality::BLURAY_1080P, true),
            ],
            min_format_score: 0,
            cutoff_format_score: 0,
            min_upgrade_format_score: 1,
            format_items: vec![],
            language: Language::ENGLISH,
            upgrade_allowed: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::profile;
    use super::*;

    #[test]
    fn index_of_quality_group_and_unknown() {
        let p = profile();
        assert_eq!(p.index_of(Quality::DVD, false).index, 1);
        assert_eq!(p.index_of(Quality::WEBDL_1080P, false).index, 2);
        assert_eq!(p.index_of(Quality::WEBDL_1080P, true).group_index, 1);
        assert_eq!(
            p.index_of_id(1001, false).index,
            2,
            "group id resolves to its row"
        );
        // Not in the profile: index 0, same as the lowest row.
        assert_eq!(
            p.index_of(Quality::REMUX_2160P, false),
            QualityIndex::default()
        );
    }

    #[test]
    fn first_and_last_allowed_skip_disallowed_rows() {
        let p = profile();
        assert_eq!(p.first_allowed_quality(), Some(Quality::DVD));
        assert_eq!(p.last_allowed_quality(), Some(Quality::BLURAY_1080P));
    }

    #[test]
    fn group_members_tie_unless_group_order_is_respected() {
        let p = profile();
        let c = p.comparer();
        assert_eq!(
            c.compare_quality(Quality::WEBDL_720P, Quality::WEBDL_1080P),
            Ordering::Equal
        );
        assert_eq!(
            c.compare_ids(Quality::WEBDL_720P.id, Quality::WEBDL_1080P.id, true),
            Ordering::Less
        );
        assert_eq!(
            c.compare_quality(Quality::BLURAY_1080P, Quality::WEBDL_1080P),
            Ordering::Greater
        );
    }

    #[test]
    fn format_score_sums_matching_items() {
        let mut p = profile();
        p.format_items = vec![
            ProfileFormatItem {
                format_id: 1,
                score: 100,
            },
            ProfileFormatItem {
                format_id: 2,
                score: -50,
            },
            ProfileFormatItem {
                format_id: 3,
                score: 7,
            },
        ];
        assert_eq!(p.calculate_custom_format_score(&[1, 2]), 50);
        assert_eq!(p.calculate_custom_format_score(&[]), 0);
        assert_eq!(p.calculate_custom_format_score(&[9]), 0);
    }
}
