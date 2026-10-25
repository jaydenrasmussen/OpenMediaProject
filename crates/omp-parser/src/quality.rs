//! Quality detection from release names and file names.
//!
//! Ported from Radarr `NzbDrone.Core/Parser/QualityParser.cs` (GPL-3.0). The
//! regexes and the order of the checks are kept as in the original, because
//! the order is the behavior: the first matching branch wins.

use std::sync::LazyLock;

use fancy_regex::Regex;
use omp_core::quality::{DetectionSource, Modifier, Quality, QualityModel, QualitySource};

use crate::files::{path_extension, quality_for_extension};

/// Compiles a pattern; all patterns here are constants, so failure is a bug.
fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|e| panic!("bad regex {pattern}: {e}"))
}

/// Joins the lines of a multi-line pattern, dropping the indentation. This
/// replaces .NET's IgnorePatternWhitespace without changing whitespace that
/// sits inside character classes.
fn joined(lines: &[&str]) -> String {
    lines.iter().map(|l| l.trim()).collect()
}

static SOURCE: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        "(?i)\\b(?:{})(?:\\b|$|[ .])",
        joined(&[
            r"(?P<bluray>M?Blu[-_. ]?Ray|HD[-_. ]?DVD|BD(?!$)|UHD2?BD|BDISO|BDMux|BD25|BD50|BR[-_. ]?DISK)|",
            r"(?P<webdl>WEB[-_. ]?DL(?:mux)?|AmazonHD|AmazonSD|iTunesHD|MaxdomeHD|NetflixU?HD|WebHD|HBOMaxHD|DisneyHD|[. ]WEB[. ](?:[xh][ .]?26[45]|AVC|HEVC|DDP?5[. ]1)|[. ](?-i:WEB)$|(?:\d{3,4}0p)[-. ](?:Hybrid[-_. ]?)?WEB[-. ]|[-. ]WEB[-. ]\d{3,4}0p|\b\s\/\sWEB\s\/\s\b|(?:AMZN|NF|DP)[. -]WEB[. -](?!Rip))|",
            r"(?P<webrip>WebRip|Web-Rip|WEBMux)|",
            r"(?P<hdtv>HDTV)|",
            r"(?P<bdrip>BDRip|BDLight|HD[-_. ]?DVDRip|UHDBDRip)|",
            r"(?P<brrip>BRRip)|",
            r"(?P<dvdr>\d?x?M?DVD-?[R59])|",
            r"(?P<dvd>DVD(?!-R)|DVDRip|xvidvd)|",
            r"(?P<dsr>WS[-_. ]DSR|DSR)|",
            r"(?P<regional>R[0-9]{1}|REGIONAL)|",
            r"(?P<scr>SCR|SCREENER|DVDSCR|DVDSCREENER)|",
            r"(?P<ts>TS[-_. ]|TELESYNCH?|HD-TS|HDTS|PDVD|TSRip|HDTSRip)|",
            r"(?P<tc>TC|TELECINE|HD-TC|HDTC)|",
            r"(?P<cam>CAMRIP|(?:NEW)?CAM|HD-?CAM(?:Rip)?|HQCAM)|",
            r"(?P<wp>WORKPRINT|WP)|",
            r"(?P<pdtv>PDTV)|",
            r"(?P<sdtv>SDTV)|",
            r"(?P<tvrip>TVRip)",
        ])
    ))
});

static RAW_HD: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?P<rawhd>RawHD|Raw[-_. ]HD)\b"));

static MPEG2: LazyLock<Regex> = LazyLock::new(|| re(r"\b(?P<mpeg2>MPEG[-_. ]?2)\b"));

static BRDISK: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)^(?!.*\b((?<!HD[._ -]|HD)DVD|BDRip|720p|MKV|XviD|WMV|d3g|(BD)?REMUX|^(?=.*1080p)(?=.*HEVC)|[xh][-_. ]?26[45]|German.*[DM]L|((?<=\d{4}).*German.*([DM]L)?)(?=.*\b(AVC|HEVC|VC[-_. ]?1|MVC|MPEG[-_. ]?2)\b))\b)(((?=.*\b(Blu[-_. ]?ray|BD|HD[-_. ]?DVD)\b)(?=.*\b(AVC|HEVC|VC[-_. ]?1|MVC|MPEG[-_. ]?2|BDMV|ISO)\b))|^((?=.*\b(((?=.*\b((.*_)?COMPLETE.*|Dis[ck])\b)(?=.*(Blu[-_. ]?ray|HD[-_. ]?DVD)))|3D[-_. ]?BD|BR[-_. ]?DISK|Full[-_. ]?Blu[-_. ]?ray|^((?=.*((BD|UHD)[-_. ]?(25|50|66|100|ISO)))))))).*",
    )
});

static PROPER: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?P<proper>proper)\b"));

static REPACK: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?P<repack>repack\d?|rerip\d?)\b"));

// .NET allowed one group name four times; Rust needs distinct names.
static VERSION: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)\d[-._ ]?v(?P<v1>\d)[-._ ]|\[v(?P<v2>\d)\]|repack(?P<v3>\d)|rerip(?P<v4>\d)")
});

static REAL: LazyLock<Regex> = LazyLock::new(|| re(r"\b(?P<real>REAL)\b"));

static RESOLUTION: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\b(?:(?P<r360p>360p)|(?P<r480p>480p|480i|640x480|848x480)|(?P<r540p>540p)|(?P<r576p>576p)|(?P<r720p>720p|1280x720|960p)|(?P<r1080p>1080p|1920x1080|1440p|FHD|1080i|4kto1080p)|(?P<r2160p>2160p|3840x2160|4k[-_. ](?:UHD|HEVC|BD|H\.?265)|(?:UHD|HEVC|BD|H\.?265)[-_. ]4k))\b",
    )
});

// Releases with no resolution at all: assume UHD means 4k.
static ALT_RESOLUTION: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(?P<a>UHD)\b|(?P<b>\[4K\])"));

static CODEC: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)\b(?:(?P<x264>x264)|(?P<h264>h264)|(?P<xvidhd>XvidHD)|(?P<xvid>X-?vid)|(?P<divx>divx))\b",
    )
});

static OTHER_SOURCE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)(?P<hdtv>HD[-_. ]TV)|(?P<sdtv>SD[-_. ]TV)"));

static ANIME_BLURAY: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)bd(?:720|1080|2160)|(?<=[-_. (\[])bd(?=[-_. )\]])"));

static ANIME_WEBDL: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\[WEB\]|[\[\(]WEB[ .]"));

static HIGH_DEF_PDTV: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)hr[-_. ]ws"));

static REMUX: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)(?:[_. \[]|\d{4}p-|\bHybrid-)(?P<remux>(?:(BD|UHD)[-_. ]?)?Remux)\b|(?P<remux2>(?:(BD|UHD)[-_. ]?)?Remux[_. ]\d{4}p)",
    )
});

static GERMAN_REMUX: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)((?<=\d{4}).*German.*([DM]L)?)(?=.*\b(AVC|HEVC|VC[_. -]?1|MVC|MPEG[_. -]?2))(?=.*Blu-?ray)",
    )
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resolution {
    Unknown,
    R360p,
    R480p,
    R540p,
    R576p,
    R720p,
    R1080p,
    R2160p,
}

impl Resolution {
    fn pixels(self) -> u16 {
        match self {
            Resolution::Unknown => 0,
            Resolution::R360p => 360,
            Resolution::R480p => 480,
            Resolution::R540p => 540,
            Resolution::R576p => 576,
            Resolution::R720p => 720,
            Resolution::R1080p => 1080,
            Resolution::R2160p => 2160,
        }
    }

    fn is_sd(self) -> bool {
        matches!(
            self,
            Resolution::R360p | Resolution::R480p | Resolution::R540p | Resolution::R576p
        )
    }
}

fn is_match(re: &Regex, text: &str) -> bool {
    re.is_match(text).unwrap_or(false)
}

fn captures<'t>(re: &Regex, text: &'t str) -> Option<fancy_regex::Captures<'t>> {
    re.captures(text).ok().flatten()
}

fn has(caps: &fancy_regex::Captures<'_>, group: &str) -> bool {
    caps.name(group).is_some()
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

/// Parses quality from a release name or file name, falling back to the file
/// extension when the name says nothing.
pub fn parse_quality(name: &str) -> QualityModel {
    if name.trim().is_empty() {
        return QualityModel::default();
    }
    let name = name.trim();
    let mut result = parse_quality_name(name);

    if result.quality == Quality::UNKNOWN && !name.contains('\0') {
        result.quality = quality_for_extension(path_extension(name));
        result.source_detection = DetectionSource::Extension;
        result.resolution_detection = DetectionSource::Extension;
        result.modifier_detection = DetectionSource::Extension;
    }
    result
}

pub fn parse_quality_name(name: &str) -> QualityModel {
    let normalized = name.replace('_', " ");
    let normalized = normalized.trim();
    let mut result = parse_quality_modifiers(name, normalized);

    // The last source match wins ("... WEB-DL ... BluRay-group" style names).
    let source = SOURCE
        .captures_iter(normalized)
        .filter_map(Result::ok)
        .last();
    let resolution = parse_resolution(normalized);
    let codec = captures(&CODEC, normalized);
    let remux = is_match(&REMUX, normalized) || is_match(&GERMAN_REMUX, normalized);
    let brdisk = is_match(&BRDISK, normalized);

    if is_match(&RAW_HD, normalized) && !brdisk {
        result.source_detection = DetectionSource::Name;
        result.resolution_detection = DetectionSource::Name;
        result.quality = Quality::RAWHD;
        return result;
    }

    if resolution != Resolution::Unknown {
        result.resolution_detection = DetectionSource::Name;
    }

    if let Some(src) = source {
        result.source_detection = DetectionSource::Name;
        let g = |n: &str| has(&src, n);

        if g("bluray") {
            if brdisk {
                result.quality = Quality::BRDISK;
                return result;
            }
            if codec
                .as_ref()
                .is_some_and(|c| has(c, "xvid") || has(c, "divx"))
            {
                result.quality = Quality::BLURAY_480P;
                return result;
            }
            result.quality = match resolution {
                Resolution::R2160p if remux => Quality::REMUX_2160P,
                Resolution::R2160p => Quality::BLURAY_2160P,
                Resolution::R1080p if remux => Quality::REMUX_1080P,
                Resolution::R1080p => Quality::BLURAY_1080P,
                Resolution::R720p => Quality::BLURAY_720P,
                Resolution::R576p => Quality::BLURAY_576P,
                Resolution::R360p | Resolution::R480p | Resolution::R540p => Quality::BLURAY_480P,
                // A remux without a resolution is treated as 1080p, not 720p.
                _ if remux => Quality::REMUX_1080P,
                _ => Quality::BLURAY_720P,
            };
            return result;
        }

        if g("webdl") {
            result.quality = match resolution {
                Resolution::R2160p => Quality::WEBDL_2160P,
                Resolution::R1080p => Quality::WEBDL_1080P,
                Resolution::R720p => Quality::WEBDL_720P,
                _ if name.contains("[WEBDL]") => Quality::WEBDL_720P,
                _ => Quality::WEBDL_480P,
            };
            return result;
        }

        if g("webrip") {
            result.quality = match resolution {
                Resolution::R2160p => Quality::WEBRIP_2160P,
                Resolution::R1080p => Quality::WEBRIP_1080P,
                Resolution::R720p => Quality::WEBRIP_720P,
                _ => Quality::WEBRIP_480P,
            };
            return result;
        }

        if g("scr") {
            result.quality = Quality::DVDSCR;
            return result;
        }
        if g("cam") {
            result.quality = Quality::CAM;
            return result;
        }
        if g("ts") {
            result.quality = Quality::TELESYNC;
            result.quality.resolution = resolution.pixels();
            return result;
        }
        if g("tc") {
            result.quality = Quality::TELECINE;
            return result;
        }
        if g("wp") {
            result.quality = Quality::WORKPRINT;
            return result;
        }
        if g("regional") {
            result.quality = Quality::REGIONAL;
            return result;
        }

        if g("hdtv") {
            if is_match(&MPEG2, normalized) {
                result.quality = Quality::RAWHD;
                return result;
            }
            result.quality = match resolution {
                Resolution::R2160p => Quality::HDTV_2160P,
                Resolution::R1080p => Quality::HDTV_1080P,
                Resolution::R720p => Quality::HDTV_720P,
                _ if name.contains("[HDTV]") => Quality::HDTV_720P,
                _ => Quality::SDTV,
            };
            return result;
        }

        if g("bdrip") || g("brrip") {
            result.quality = match resolution {
                Resolution::R720p => Quality::BLURAY_720P,
                Resolution::R1080p => Quality::BLURAY_1080P,
                Resolution::R2160p => Quality::BLURAY_2160P,
                Resolution::R576p => Quality::BLURAY_576P,
                _ => Quality::BLURAY_480P,
            };
            return result;
        }

        if g("dvdr") {
            result.quality = Quality::DVDR;
            return result;
        }
        if g("dvd") {
            result.quality = Quality::DVD;
            return result;
        }

        if g("pdtv") || g("sdtv") || g("dsr") || g("tvrip") {
            if resolution == Resolution::R1080p || contains_ci(normalized, "1080p") {
                result.quality = Quality::HDTV_1080P;
                return result;
            }
            if resolution == Resolution::R720p || contains_ci(normalized, "720p") {
                result.quality = Quality::HDTV_720P;
                return result;
            }
            if is_match(&HIGH_DEF_PDTV, normalized) {
                result.resolution_detection = DetectionSource::Name;
                result.quality = Quality::HDTV_720P;
                return result;
            }
            result.quality = Quality::SDTV;
            return result;
        }
    }

    if source_is_none(normalized) && remux && resolution != Resolution::Unknown {
        result.source_detection = DetectionSource::Unknown;
        match resolution {
            Resolution::R480p => {
                result.quality = Quality::BLURAY_480P;
                return result;
            }
            Resolution::R720p => {
                result.quality = Quality::BLURAY_720P;
                return result;
            }
            Resolution::R2160p => {
                result.quality = Quality::REMUX_2160P;
                return result;
            }
            Resolution::R1080p => {
                result.quality = Quality::REMUX_1080P;
                return result;
            }
            _ => {}
        }
    }

    // Anime Bluray matching
    if is_match(&ANIME_BLURAY, normalized) {
        result.source_detection = DetectionSource::Name;

        if resolution.is_sd() || contains_ci(normalized, "480p") {
            result.resolution_detection = DetectionSource::Name;
            result.quality = Quality::DVD;
            return result;
        }
        if resolution == Resolution::R1080p || contains_ci(normalized, "1080p") {
            result.resolution_detection = DetectionSource::Name;
            result.quality = if remux {
                Quality::REMUX_1080P
            } else {
                Quality::BLURAY_1080P
            };
            return result;
        }
        if resolution == Resolution::R2160p || contains_ci(normalized, "2160p") {
            result.resolution_detection = DetectionSource::Name;
            result.quality = if remux {
                Quality::REMUX_2160P
            } else {
                Quality::BLURAY_2160P
            };
            return result;
        }
        if remux && resolution != Resolution::R720p {
            result.quality = Quality::REMUX_1080P;
            return result;
        }
        result.quality = Quality::BLURAY_720P;
        return result;
    }

    if is_match(&ANIME_WEBDL, normalized) {
        result.source_detection = DetectionSource::Name;

        if resolution.is_sd() || contains_ci(normalized, "480p") {
            result.resolution_detection = DetectionSource::Name;
            result.quality = Quality::WEBDL_480P;
            return result;
        }
        if resolution == Resolution::R1080p || contains_ci(normalized, "1080p") {
            result.resolution_detection = DetectionSource::Name;
            result.quality = Quality::WEBDL_1080P;
            return result;
        }
        if resolution == Resolution::R2160p || contains_ci(normalized, "2160p") {
            result.resolution_detection = DetectionSource::Name;
            result.quality = Quality::WEBDL_2160P;
            return result;
        }
        result.quality = Quality::WEBDL_720P;
        return result;
    }

    if resolution != Resolution::Unknown {
        let mut source = QualitySource::Unknown;
        let mut modifier = Modifier::None;

        if remux {
            result.source_detection = DetectionSource::Name;
            source = QualitySource::Bluray;
            modifier = Modifier::Remux;
        } else {
            let from_ext = quality_for_extension(path_extension(name));
            if from_ext != Quality::UNKNOWN {
                result.source_detection = DetectionSource::Extension;
                source = from_ext.source;
            }
        }

        let (pixels, default) = match resolution {
            Resolution::R2160p => (2160, Quality::HDTV_2160P),
            Resolution::R1080p => (1080, Quality::HDTV_1080P),
            Resolution::R720p => (720, Quality::HDTV_720P),
            _ => (480, Quality::SDTV),
        };
        result.resolution_detection = DetectionSource::Name;
        result.quality = if source == QualitySource::Unknown {
            default
        } else {
            Quality::find_by_source_and_resolution(source, pixels, modifier)
        };
        return result;
    }

    if codec.as_ref().is_some_and(|c| has(c, "x264")) {
        result.quality = Quality::SDTV;
        return result;
    }

    if contains_ci(normalized, "848x480") {
        result.resolution_detection = DetectionSource::Name;
        if normalized.contains("dvd") {
            result.source_detection = DetectionSource::Name;
            result.quality = Quality::DVD;
        } else if contains_ci(normalized, "bluray") {
            result.source_detection = DetectionSource::Name;
            result.quality = Quality::BLURAY_480P;
        } else {
            result.quality = Quality::SDTV;
        }
        return result;
    }

    if contains_ci(normalized, "1280x720") {
        result.resolution_detection = DetectionSource::Name;
        if contains_ci(normalized, "bluray") {
            result.source_detection = DetectionSource::Name;
            result.quality = Quality::BLURAY_720P;
        } else {
            result.quality = Quality::HDTV_720P;
        }
        return result;
    }

    if contains_ci(normalized, "1920x1080") {
        result.resolution_detection = DetectionSource::Name;
        if contains_ci(normalized, "bluray") {
            result.source_detection = DetectionSource::Name;
            result.quality = Quality::BLURAY_1080P;
        } else {
            result.quality = Quality::HDTV_1080P;
        }
        return result;
    }

    for (needle, quality) in [
        ("bluray720p", Quality::BLURAY_720P),
        ("bluray1080p", Quality::BLURAY_1080P),
        ("bluray2160p", Quality::BLURAY_2160P),
    ] {
        if contains_ci(normalized, needle) {
            result.source_detection = DetectionSource::Name;
            result.resolution_detection = DetectionSource::Name;
            result.quality = quality;
            return result;
        }
    }

    if let Some(other) = captures(&OTHER_SOURCE, normalized) {
        result.source_detection = DetectionSource::Name;
        result.quality = if has(&other, "sdtv") {
            Quality::SDTV
        } else {
            Quality::HDTV_720P
        };
    }

    result
}

/// True when the source regex finds nothing in the name.
fn source_is_none(normalized: &str) -> bool {
    SOURCE
        .captures_iter(normalized)
        .filter_map(Result::ok)
        .next()
        .is_none()
}

fn parse_resolution(name: &str) -> Resolution {
    let main = captures(&RESOLUTION, name);
    let alt = captures(&ALT_RESOLUTION, name);
    if main.is_none() && alt.is_none() {
        return Resolution::Unknown;
    }
    if let Some(m) = &main {
        for (group, res) in [
            ("r360p", Resolution::R360p),
            ("r480p", Resolution::R480p),
            ("r540p", Resolution::R540p),
            ("r576p", Resolution::R576p),
            ("r720p", Resolution::R720p),
            ("r1080p", Resolution::R1080p),
            ("r2160p", Resolution::R2160p),
        ] {
            if has(m, group) {
                return res;
            }
        }
    }
    if alt.is_some() {
        return Resolution::R2160p;
    }
    Resolution::Unknown
}

fn parse_quality_modifiers(name: &str, normalized: &str) -> QualityModel {
    let mut result = QualityModel::default();

    let version = captures(&VERSION, normalized).and_then(|c| {
        ["v1", "v2", "v3", "v4"]
            .iter()
            .find_map(|g| c.name(g))
            .and_then(|m| m.as_str().parse::<u32>().ok())
    });

    if let Some(v) = version {
        result.revision.version = v;
        result.revision_detection = DetectionSource::Name;
    }
    if is_match(&PROPER, normalized) {
        result.revision.version = version.map_or(2, |v| v + 1);
        result.revision_detection = DetectionSource::Name;
    }
    if is_match(&REPACK, normalized) {
        result.revision.version = version.map_or(2, |v| v + 1);
        result.revision.is_repack = true;
        result.revision_detection = DetectionSource::Name;
    }

    let reals = REAL.find_iter(name).filter_map(Result::ok).count();
    if reals > 0 {
        result.revision.real = reals as u32;
        result.revision_detection = DetectionSource::Name;
    }

    result
}
