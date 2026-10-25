//! Per-quality size limits, in megabytes per minute of runtime.

use omp_core::quality::Quality;

#[derive(Debug, Clone, PartialEq)]
pub struct QualityDefinition {
    pub quality: Quality,
    /// Qualities with the same weight rank together (WEB-DL and WEBRip).
    pub weight: u32,
    pub min_size: Option<f64>,
    /// `None` or 0 means unlimited.
    pub max_size: Option<f64>,
    pub preferred_size: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct QualityDefinitions(Vec<QualityDefinition>);

impl QualityDefinitions {
    pub fn new(definitions: Vec<QualityDefinition>) -> Self {
        Self(definitions)
    }

    /// Radarr's defaults.
    pub fn defaults() -> Self {
        Self(default_definitions())
    }

    pub fn get(&self, quality: Quality) -> Option<&QualityDefinition> {
        self.0.iter().find(|d| d.quality == quality)
    }

    pub fn all(&self) -> &[QualityDefinition] {
        &self.0
    }
}

/// Radarr's default definitions: no minimum, a 100 MB/min cap and a 95 MB/min
/// preferred size up to WEB 1080p, and no limits above that.
fn default_definitions() -> Vec<QualityDefinition> {
    let limited = |weight, quality| QualityDefinition {
        quality,
        weight,
        min_size: Some(0.0),
        max_size: Some(100.0),
        preferred_size: Some(95.0),
    };
    let unlimited = |weight, quality| QualityDefinition {
        quality,
        weight,
        min_size: Some(0.0),
        max_size: None,
        preferred_size: None,
    };
    vec![
        limited(1, Quality::UNKNOWN),
        limited(2, Quality::WORKPRINT),
        limited(3, Quality::CAM),
        limited(4, Quality::TELESYNC),
        limited(5, Quality::TELECINE),
        limited(6, Quality::REGIONAL),
        limited(7, Quality::DVDSCR),
        limited(8, Quality::SDTV),
        limited(9, Quality::DVD),
        limited(10, Quality::DVDR),
        limited(11, Quality::WEBDL_480P),
        limited(11, Quality::WEBRIP_480P),
        limited(12, Quality::BLURAY_480P),
        limited(13, Quality::BLURAY_576P),
        limited(14, Quality::HDTV_720P),
        limited(15, Quality::WEBDL_720P),
        limited(15, Quality::WEBRIP_720P),
        limited(16, Quality::BLURAY_720P),
        limited(17, Quality::HDTV_1080P),
        limited(18, Quality::WEBDL_1080P),
        limited(18, Quality::WEBRIP_1080P),
        unlimited(19, Quality::BLURAY_1080P),
        unlimited(20, Quality::REMUX_1080P),
        unlimited(21, Quality::HDTV_2160P),
        unlimited(22, Quality::WEBDL_2160P),
        unlimited(22, Quality::WEBRIP_2160P),
        unlimited(23, Quality::BLURAY_2160P),
        unlimited(24, Quality::REMUX_2160P),
        unlimited(25, Quality::BRDISK),
        unlimited(26, Quality::RAWHD),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_quality_has_a_definition() {
        let defs = QualityDefinitions::defaults();
        for q in Quality::ALL {
            assert!(defs.get(q).is_some(), "{q} has no definition");
        }
        assert_eq!(defs.all().len(), Quality::ALL.len());
    }

    #[test]
    fn limits_follow_radarr() {
        let defs = QualityDefinitions::defaults();
        let web = defs.get(Quality::WEBDL_1080P).unwrap();
        assert_eq!(
            (web.min_size, web.max_size, web.preferred_size),
            (Some(0.0), Some(100.0), Some(95.0))
        );
        let remux = defs.get(Quality::REMUX_2160P).unwrap();
        assert_eq!((remux.max_size, remux.preferred_size), (None, None));
        assert_eq!(
            defs.get(Quality::WEBDL_720P).unwrap().weight,
            defs.get(Quality::WEBRIP_720P).unwrap().weight
        );
    }
}
