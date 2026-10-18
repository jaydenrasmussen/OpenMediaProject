//! The profiles a fresh install starts with. Ported from Radarr
//! `QualityProfileService` (GPL-3.0).

use omp_core::{language::Language, quality::Quality};

use crate::{
    definition::QualityDefinitions,
    profile::{ProfileItem, QualityProfile},
};

/// Builds a profile over every known quality: those in `allowed` are on, in
/// definition order. Qualities that share a weight (WEB-DL / WEBRip) form a
/// group named like "WEB 1080p". `cutoff` falls back to Unknown.
pub fn default_profile(name: &str, cutoff: Option<Quality>, allowed: &[Quality]) -> QualityProfile {
    let definitions = QualityDefinitions::defaults();
    let mut items = Vec::new();
    let mut next_group_id = 1000;
    let mut profile_cutoff = cutoff.map_or(Quality::UNKNOWN.id, |q| q.id);

    let mut weights: Vec<u32> = definitions.all().iter().map(|d| d.weight).collect();
    weights.dedup();
    for weight in weights {
        let group: Vec<Quality> = definitions
            .all()
            .iter()
            .filter(|d| d.weight == weight)
            .map(|d| d.quality)
            .collect();
        if let [only] = group[..] {
            items.push(ProfileItem::Quality {
                quality: only,
                allowed: allowed.contains(&only),
            });
            continue;
        }
        let group_allowed = group.iter().any(|q| allowed.contains(q));
        if cutoff.is_some_and(|c| group.contains(&c)) {
            profile_cutoff = next_group_id;
        }
        items.push(ProfileItem::Group {
            id: next_group_id,
            name: format!("WEB {}p", group[0].resolution),
            qualities: group,
            allowed: group_allowed,
        });
        next_group_id += 1;
    }

    QualityProfile {
        id: 0,
        name: name.to_owned(),
        cutoff: profile_cutoff,
        items,
        min_format_score: 0,
        cutoff_format_score: 0,
        min_upgrade_format_score: 1,
        format_items: Vec::new(),
        language: Language::ORIGINAL,
        upgrade_allowed: true,
    }
}

/// Any, SD, HD-720p, HD-1080p, Ultra-HD and HD - 720p/1080p.
pub fn default_profiles() -> Vec<QualityProfile> {
    use Quality as Q;
    vec![
        default_profile(
            "Any",
            Some(Q::BLURAY_480P),
            &[
                Q::BLURAY_480P,
                Q::WORKPRINT,
                Q::CAM,
                Q::TELESYNC,
                Q::TELECINE,
                Q::DVDSCR,
                Q::REGIONAL,
                Q::SDTV,
                Q::DVD,
                Q::DVDR,
                Q::HDTV_720P,
                Q::HDTV_1080P,
                Q::HDTV_2160P,
                Q::WEBDL_480P,
                Q::WEBRIP_480P,
                Q::WEBDL_720P,
                Q::WEBRIP_720P,
                Q::WEBDL_1080P,
                Q::WEBRIP_1080P,
                Q::WEBDL_2160P,
                Q::WEBRIP_2160P,
                Q::BLURAY_480P,
                Q::BLURAY_576P,
                Q::BLURAY_720P,
                Q::BLURAY_1080P,
                Q::BLURAY_2160P,
                Q::REMUX_1080P,
                Q::REMUX_2160P,
                Q::BRDISK,
            ],
        ),
        default_profile(
            "SD",
            Some(Q::BLURAY_480P),
            &[
                Q::BLURAY_480P,
                Q::WORKPRINT,
                Q::CAM,
                Q::TELESYNC,
                Q::TELECINE,
                Q::DVDSCR,
                Q::REGIONAL,
                Q::SDTV,
                Q::DVD,
                Q::WEBDL_480P,
                Q::WEBRIP_480P,
                Q::BLURAY_480P,
                Q::BLURAY_576P,
            ],
        ),
        default_profile(
            "HD-720p",
            Some(Q::BLURAY_720P),
            &[Q::BLURAY_720P, Q::HDTV_720P, Q::WEBDL_720P, Q::WEBRIP_720P],
        ),
        default_profile(
            "HD-1080p",
            Some(Q::BLURAY_1080P),
            &[
                Q::BLURAY_1080P,
                Q::HDTV_1080P,
                Q::WEBDL_1080P,
                Q::WEBRIP_1080P,
                Q::REMUX_1080P,
            ],
        ),
        default_profile(
            "Ultra-HD",
            Some(Q::REMUX_2160P),
            &[
                Q::REMUX_2160P,
                Q::HDTV_2160P,
                Q::WEBDL_2160P,
                Q::WEBRIP_2160P,
                Q::BLURAY_2160P,
            ],
        ),
        default_profile(
            "HD - 720p/1080p",
            Some(Q::BLURAY_720P),
            &[
                Q::BLURAY_720P,
                Q::HDTV_720P,
                Q::HDTV_1080P,
                Q::WEBDL_720P,
                Q::WEBRIP_720P,
                Q::WEBDL_1080P,
                Q::WEBRIP_1080P,
                Q::BLURAY_1080P,
                Q::REMUX_1080P,
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_qualities_group_and_the_cutoff_follows() {
        let p = default_profile(
            "t",
            Some(Quality::WEBDL_1080P),
            &[Quality::WEBDL_1080P, Quality::BLURAY_1080P],
        );
        let group = p
            .items
            .iter()
            .find_map(|i| match i {
                ProfileItem::Group {
                    id,
                    name,
                    qualities,
                    allowed,
                } if name == "WEB 1080p" => Some((*id, qualities.len(), *allowed)),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            (group.1, group.2),
            (2, true),
            "WEBDL and WEBRip together, on because one is allowed"
        );
        assert_eq!(
            p.cutoff, group.0,
            "a cutoff inside a group becomes the group's id"
        );
        assert!(p.items.iter().any(|i| matches!(i, ProfileItem::Quality { quality, allowed: true } if *quality == Quality::BLURAY_1080P)));
    }

    #[test]
    fn validation_catches_broken_profiles() {
        let good = default_profile("t", Some(Quality::BLURAY_1080P), &[Quality::BLURAY_1080P]);
        assert_eq!(good.validate(), Ok(()));

        let mut missing = good.clone();
        missing.items.pop();
        assert!(missing.validate().unwrap_err().contains("exactly once"));

        let mut nothing = good.clone();
        for i in &mut nothing.items {
            if let ProfileItem::Quality { allowed, .. } = i {
                *allowed = false;
            }
        }
        assert!(nothing.validate().unwrap_err().contains("allowed"));

        let mut bad_cutoff = good.clone();
        bad_cutoff.cutoff = Quality::DVD.id; // present, but not allowed
        assert!(bad_cutoff.validate().unwrap_err().contains("cutoff"));
        bad_cutoff.cutoff = 4242;
        assert!(bad_cutoff.validate().unwrap_err().contains("cutoff"));

        let mut unnamed = good;
        unnamed.name = "  ".into();
        assert!(unnamed.validate().is_err());
    }

    #[test]
    fn six_default_profiles_with_sane_cutoffs() {
        let all = default_profiles();
        assert_eq!(
            all.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            [
                "Any",
                "SD",
                "HD-720p",
                "HD-1080p",
                "Ultra-HD",
                "HD - 720p/1080p"
            ]
        );
        for p in &all {
            assert!(
                p.items.iter().any(ProfileItem::allowed),
                "{} allows something",
                p.name
            );
            let cutoff_present = p.items.iter().any(|i| match i {
                ProfileItem::Quality { quality, .. } => quality.id == p.cutoff,
                ProfileItem::Group { id, .. } => *id == p.cutoff,
            });
            assert!(
                cutoff_present,
                "{} cutoff {} is in its items",
                p.name, p.cutoff
            );
        }
        let hd = &all[3];
        assert_eq!(hd.cutoff, Quality::BLURAY_1080P.id);
        assert!(!hd.items.iter().any(|i| matches!(i, ProfileItem::Quality { quality, allowed: true } if *quality == Quality::DVD)));
    }
}
