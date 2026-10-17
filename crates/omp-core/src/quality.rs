//! Quality model.
//!
//! A quality is a (source, resolution, modifier) triple with a stable id. The
//! ids and names follow Radarr's table, which is a superset of Sonarr's
//! (Sonarr's "Bluray-1080p Remux" is our `REMUX_1080P`). Ids 11 and 13 are
//! unused; they belonged to qualities Radarr dropped.
//!
//! Ported from Radarr `NzbDrone.Core/Qualities` (GPL-3.0).

use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Declaration order matters: `QualityFinder` compares sources with `>=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QualitySource {
    Unknown,
    Cam,
    Telesync,
    Telecine,
    Workprint,
    Dvd,
    Tv,
    WebDl,
    WebRip,
    Bluray,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    None,
    Regional,
    Screener,
    RawHd,
    BrDisk,
    Remux,
}

/// A named quality. Equality and hashing use the id only, so a parser that
/// adjusts `resolution` (telesync releases do) still compares equal.
#[derive(Debug, Clone, Copy)]
pub struct Quality {
    pub id: u32,
    pub name: &'static str,
    pub source: QualitySource,
    /// Vertical resolution in pixels; 0 when unknown.
    pub resolution: u16,
    pub modifier: Modifier,
}

impl PartialEq for Quality {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for Quality {}
impl std::hash::Hash for Quality {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}
/// Stored and sent as the quality id, like Radarr's JSON.
impl Serialize for Quality {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.id)
    }
}

impl<'de> Deserialize<'de> for Quality {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let id = u32::deserialize(deserializer)?;
        Quality::from_id(id)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown quality id {id}")))
    }
}

impl fmt::Display for Quality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

const fn q(
    id: u32,
    name: &'static str,
    source: QualitySource,
    resolution: u16,
    modifier: Modifier,
) -> Quality {
    Quality {
        id,
        name,
        source,
        resolution,
        modifier,
    }
}

use Modifier as M;
use QualitySource as S;

impl Quality {
    pub const UNKNOWN: Quality = q(0, "Unknown", S::Unknown, 0, M::None);
    pub const WORKPRINT: Quality = q(24, "WORKPRINT", S::Workprint, 0, M::None);
    pub const CAM: Quality = q(25, "CAM", S::Cam, 0, M::None);
    pub const TELESYNC: Quality = q(26, "TELESYNC", S::Telesync, 0, M::None);
    pub const TELECINE: Quality = q(27, "TELECINE", S::Telecine, 0, M::None);
    pub const DVDSCR: Quality = q(28, "DVDSCR", S::Dvd, 480, M::Screener);
    pub const REGIONAL: Quality = q(29, "REGIONAL", S::Dvd, 480, M::Regional);
    pub const SDTV: Quality = q(1, "SDTV", S::Tv, 480, M::None);
    pub const DVD: Quality = q(2, "DVD", S::Dvd, 0, M::None);
    pub const DVDR: Quality = q(23, "DVD-R", S::Dvd, 480, M::Remux);
    pub const HDTV_720P: Quality = q(4, "HDTV-720p", S::Tv, 720, M::None);
    pub const HDTV_1080P: Quality = q(9, "HDTV-1080p", S::Tv, 1080, M::None);
    pub const HDTV_2160P: Quality = q(16, "HDTV-2160p", S::Tv, 2160, M::None);
    pub const WEBDL_480P: Quality = q(8, "WEBDL-480p", S::WebDl, 480, M::None);
    pub const WEBDL_720P: Quality = q(5, "WEBDL-720p", S::WebDl, 720, M::None);
    pub const WEBDL_1080P: Quality = q(3, "WEBDL-1080p", S::WebDl, 1080, M::None);
    pub const WEBDL_2160P: Quality = q(18, "WEBDL-2160p", S::WebDl, 2160, M::None);
    pub const WEBRIP_480P: Quality = q(12, "WEBRip-480p", S::WebRip, 480, M::None);
    pub const WEBRIP_720P: Quality = q(14, "WEBRip-720p", S::WebRip, 720, M::None);
    pub const WEBRIP_1080P: Quality = q(15, "WEBRip-1080p", S::WebRip, 1080, M::None);
    pub const WEBRIP_2160P: Quality = q(17, "WEBRip-2160p", S::WebRip, 2160, M::None);
    pub const BLURAY_480P: Quality = q(20, "Bluray-480p", S::Bluray, 480, M::None);
    pub const BLURAY_576P: Quality = q(21, "Bluray-576p", S::Bluray, 576, M::None);
    pub const BLURAY_720P: Quality = q(6, "Bluray-720p", S::Bluray, 720, M::None);
    pub const BLURAY_1080P: Quality = q(7, "Bluray-1080p", S::Bluray, 1080, M::None);
    pub const BLURAY_2160P: Quality = q(19, "Bluray-2160p", S::Bluray, 2160, M::None);
    pub const REMUX_1080P: Quality = q(30, "Remux-1080p", S::Bluray, 1080, M::Remux);
    pub const REMUX_2160P: Quality = q(31, "Remux-2160p", S::Bluray, 2160, M::Remux);
    pub const BRDISK: Quality = q(22, "BR-DISK", S::Bluray, 1080, M::BrDisk);
    pub const RAWHD: Quality = q(10, "Raw-HD", S::Tv, 1080, M::RawHd);

    /// Every quality, in id order.
    pub const ALL: [Quality; 30] = [
        Self::UNKNOWN,
        Self::SDTV,
        Self::DVD,
        Self::WEBDL_1080P,
        Self::HDTV_720P,
        Self::WEBDL_720P,
        Self::BLURAY_720P,
        Self::BLURAY_1080P,
        Self::WEBDL_480P,
        Self::HDTV_1080P,
        Self::RAWHD,
        Self::WEBRIP_480P,
        Self::WEBRIP_720P,
        Self::WEBRIP_1080P,
        Self::HDTV_2160P,
        Self::WEBRIP_2160P,
        Self::WEBDL_2160P,
        Self::BLURAY_2160P,
        Self::BLURAY_480P,
        Self::BLURAY_576P,
        Self::BRDISK,
        Self::DVDR,
        Self::WORKPRINT,
        Self::CAM,
        Self::TELESYNC,
        Self::TELECINE,
        Self::DVDSCR,
        Self::REGIONAL,
        Self::REMUX_1080P,
        Self::REMUX_2160P,
    ];

    pub fn from_id(id: u32) -> Option<Quality> {
        Self::ALL.iter().copied().find(|q| q.id == id)
    }

    /// Port of Radarr's `QualityFinder.FindBySourceAndResolution`: exact
    /// (source, resolution, modifier) match, else a source+modifier match
    /// among qualities with unknown resolution, else the nearest source at
    /// the same resolution and modifier, else `UNKNOWN`.
    pub fn find_by_source_and_resolution(
        source: QualitySource,
        resolution: u16,
        modifier: Modifier,
    ) -> Quality {
        if let Some(exact) = Self::ALL
            .iter()
            .find(|q| q.source == source && q.resolution == resolution && q.modifier == modifier)
        {
            return *exact;
        }

        let unknown_res: Vec<Quality> = Self::ALL
            .iter()
            .copied()
            .filter(|q| {
                q.source == source
                    && q.resolution == 0
                    && q.modifier == modifier
                    && *q != Self::UNKNOWN
            })
            .collect();
        if let [only] = unknown_res.as_slice() {
            return *only;
        }
        if let Some(found) = unknown_res.iter().find(|q| q.source >= source) {
            return *found;
        }

        let mut same_res: Vec<Quality> = Self::ALL
            .iter()
            .copied()
            .filter(|q| q.modifier == modifier && q.resolution == resolution)
            .collect();
        same_res.sort_by_key(|q| q.source);
        same_res
            .into_iter()
            .find(|q| q.source >= source)
            .unwrap_or(Self::UNKNOWN)
    }
}

/// Release revision: proper/repack/real counters. Higher wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision {
    pub version: u32,
    pub real: u32,
    pub is_repack: bool,
}

impl Default for Revision {
    fn default() -> Self {
        Self {
            version: 1,
            real: 0,
            is_repack: false,
        }
    }
}

impl PartialOrd for Revision {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(
            self.real
                .cmp(&other.real)
                .then(self.version.cmp(&other.version)),
        )
    }
}

/// Where a part of the quality was detected from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetectionSource {
    #[default]
    Unknown,
    Name,
    Extension,
    MediaInfo,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualityModel {
    pub quality: Quality,
    pub revision: Revision,
    pub source_detection: DetectionSource,
    pub resolution_detection: DetectionSource,
    pub modifier_detection: DetectionSource,
    pub revision_detection: DetectionSource,
}

/// Only the quality and revision are stored; where each part was detected
/// from is a parsing detail.
#[derive(Serialize, Deserialize)]
struct StoredQualityModel {
    quality: Quality,
    #[serde(default)]
    revision: Revision,
}

impl Serialize for QualityModel {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        StoredQualityModel {
            quality: self.quality,
            revision: self.revision,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for QualityModel {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stored = StoredQualityModel::deserialize(deserializer)?;
        Ok(QualityModel {
            quality: stored.quality,
            revision: stored.revision,
            ..Default::default()
        })
    }
}

impl Default for QualityModel {
    fn default() -> Self {
        Self {
            quality: Quality::UNKNOWN,
            revision: Revision::default(),
            source_detection: DetectionSource::Unknown,
            resolution_detection: DetectionSource::Unknown,
            modifier_detection: DetectionSource::Unknown,
            revision_detection: DetectionSource::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_all_resolve() {
        let mut ids: Vec<_> = Quality::ALL.iter().map(|q| q.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), Quality::ALL.len());
        for q in Quality::ALL {
            assert_eq!(Quality::from_id(q.id), Some(q));
        }
    }

    #[test]
    fn finder_exact_and_fallback() {
        assert_eq!(
            Quality::find_by_source_and_resolution(S::Bluray, 1080, M::Remux),
            Quality::REMUX_1080P
        );
        assert_eq!(
            Quality::find_by_source_and_resolution(S::Tv, 720, M::None),
            Quality::HDTV_720P
        );
        // Only one DVD-with-unknown-resolution quality exists.
        assert_eq!(
            Quality::find_by_source_and_resolution(S::Dvd, 480, M::None),
            Quality::DVD
        );
    }

    #[test]
    fn quality_model_round_trips_through_json() {
        let model = QualityModel {
            quality: Quality::REMUX_1080P,
            revision: Revision {
                version: 2,
                real: 1,
                is_repack: true,
            },
            source_detection: DetectionSource::Name,
            ..Default::default()
        };
        let json = serde_json::to_string(&model).unwrap();
        assert_eq!(
            json,
            r#"{"quality":30,"revision":{"version":2,"real":1,"is_repack":true}}"#
        );
        let back: QualityModel = serde_json::from_str(&json).unwrap();
        assert_eq!(back.quality, Quality::REMUX_1080P);
        assert_eq!(back.revision, model.revision);
        assert_eq!(
            back.source_detection,
            DetectionSource::Unknown,
            "detection is not stored"
        );
        assert!(serde_json::from_str::<Quality>("999").is_err());
    }

    #[test]
    fn revision_orders_real_before_version() {
        let a = Revision {
            version: 3,
            real: 0,
            is_repack: false,
        };
        let b = Revision {
            version: 1,
            real: 1,
            is_repack: false,
        };
        assert!(b > a);
    }
}
