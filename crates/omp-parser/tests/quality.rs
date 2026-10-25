//! Runs the quality cases extracted from Radarr's parser tests
//! (`tests/data/*.tsv`, see the header line of each file).

use omp_core::quality::{DetectionSource, Modifier, Quality, QualitySource};
use omp_parser::quality::parse_quality;

fn rows(file: &str) -> Vec<Vec<String>> {
    let path = format!("{}/tests/data/{file}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| l.split('\t').map(str::to_owned).collect())
        .collect()
}

fn source(s: &str) -> QualitySource {
    match s {
        "UNKNOWN" => QualitySource::Unknown,
        "CAM" => QualitySource::Cam,
        "TELESYNC" => QualitySource::Telesync,
        "TELECINE" => QualitySource::Telecine,
        "WORKPRINT" => QualitySource::Workprint,
        "DVD" => QualitySource::Dvd,
        "TV" => QualitySource::Tv,
        "WEBDL" => QualitySource::WebDl,
        "WEBRIP" => QualitySource::WebRip,
        "BLURAY" => QualitySource::Bluray,
        other => panic!("source {other}"),
    }
}

fn modifier(s: &str) -> Modifier {
    match s {
        "NONE" => Modifier::None,
        "REGIONAL" => Modifier::Regional,
        "SCREENER" => Modifier::Screener,
        "RAWHD" => Modifier::RawHd,
        "BRDISK" => Modifier::BrDisk,
        "REMUX" => Modifier::Remux,
        other => panic!("modifier {other}"),
    }
}

fn resolution(s: &str) -> Option<u16> {
    match s {
        "Unknown" => None,
        r => Some(
            r.trim_start_matches('R')
                .trim_end_matches('p')
                .parse()
                .unwrap(),
        ),
    }
}

fn detection(s: &str) -> DetectionSource {
    match s {
        "Unknown" => DetectionSource::Unknown,
        "Name" => DetectionSource::Name,
        "Extension" => DetectionSource::Extension,
        other => panic!("detection {other}"),
    }
}

/// Collects every failing case so one run shows them all.
fn check(failures: &mut Vec<String>, title: &str, ok: bool, detail: impl FnOnce() -> String) {
    if !ok {
        failures.push(format!("{title:?}: {}", detail()));
    }
}

#[test]
fn quality_from_name() {
    let mut failures = Vec::new();
    let all = rows("quality_verify.tsv");
    let total = all.len();
    assert!(total >= 240, "corpus unexpectedly small: {total}");
    for r in all {
        let (title, src, res, modi, proper) = (
            &r[0],
            source(&r[1]),
            resolution(&r[2]),
            modifier(&r[3]),
            r[4] == "True",
        );
        let got = parse_quality(title);
        let mut bad = Vec::new();
        if let Some(px) = res
            && got.quality.resolution != px
        {
            bad.push(format!("resolution {} != {px}", got.quality.resolution));
        }
        if got.quality.source != src {
            bad.push(format!("source {:?} != {src:?}", got.quality.source));
        }
        if modi != Modifier::None && got.quality.modifier != modi {
            bad.push(format!("modifier {:?} != {modi:?}", got.quality.modifier));
        }
        let version = if proper { 2 } else { 1 };
        if got.revision.version != version {
            bad.push(format!("version {} != {version}", got.revision.version));
        }
        check(&mut failures, title, bad.is_empty(), || {
            format!("{} ({})", bad.join(", "), got.quality)
        });
    }
    assert!(
        failures.is_empty(),
        "{} of {total} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn repack_versions() {
    for r in rows("quality_repack.tsv") {
        let got = parse_quality(&r[0]);
        assert_eq!(got.revision.is_repack, r[1] == "true", "{}", r[0]);
        assert_eq!(
            got.revision.version,
            r[2].parse::<u32>().unwrap(),
            "{}",
            r[0]
        );
    }
}

#[test]
fn detection_sources() {
    for r in rows("quality_detect.tsv") {
        let got = parse_quality(&r[0]);
        assert_eq!(got.source_detection, detection(&r[1]), "source: {}", r[0]);
        assert_eq!(
            got.resolution_detection,
            detection(&r[2]),
            "resolution: {}",
            r[0]
        );
    }
}

#[test]
fn windows_path_in_name() {
    // The one case the extractor skipped (verbatim C# string).
    let got =
        parse_quality(r"E:\Downloads\tv\Movie.Name.S01E01.720p.HDTV\ajifajjjeaeaeqwer_eppj.avi");
    assert_eq!(got.quality, Quality::HDTV_720P);
}

#[test]
fn empty_and_unknown() {
    assert_eq!(parse_quality("").quality, Quality::UNKNOWN);
    assert_eq!(parse_quality("   ").quality, Quality::UNKNOWN);
    assert_eq!(parse_quality("just some words").quality, Quality::UNKNOWN);
}

#[test]
fn other_source_spellings() {
    // Radarr's should_parse_quality_from_other_source: every separator style.
    let cases = [
        ("SD TV", QualitySource::Tv, Some(480), Modifier::None),
        ("SD DVD", QualitySource::Dvd, None, Modifier::None),
        (
            "480p WEB-DL",
            QualitySource::WebDl,
            Some(480),
            Modifier::None,
        ),
        ("HD TV", QualitySource::Tv, Some(720), Modifier::None),
        ("1080p HD TV", QualitySource::Tv, Some(1080), Modifier::None),
        ("2160p HD TV", QualitySource::Tv, Some(2160), Modifier::None),
        (
            "720p WEB-DL",
            QualitySource::WebDl,
            Some(720),
            Modifier::None,
        ),
        (
            "1080p WEB-DL",
            QualitySource::WebDl,
            Some(1080),
            Modifier::None,
        ),
        (
            "2160p WEB-DL",
            QualitySource::WebDl,
            Some(2160),
            Modifier::None,
        ),
        (
            "720p BluRay",
            QualitySource::Bluray,
            Some(720),
            Modifier::None,
        ),
        (
            "1080p BluRay",
            QualitySource::Bluray,
            Some(1080),
            Modifier::None,
        ),
        (
            "2160p BluRay",
            QualitySource::Bluray,
            Some(2160),
            Modifier::None,
        ),
        (
            "1080p Remux",
            QualitySource::Bluray,
            Some(1080),
            Modifier::Remux,
        ),
        (
            "2160p Remux",
            QualitySource::Bluray,
            Some(2160),
            Modifier::Remux,
        ),
    ];
    for (text, src, res, modi) in cases {
        for sep in ['-', '.', ' ', '_'] {
            let title = format!("My series S01E01 {}", text.replace(' ', &sep.to_string()));
            let got = parse_quality(&title);
            assert_eq!(got.quality.source, src, "{title}");
            if let Some(px) = res {
                assert_eq!(got.quality.resolution, px, "{title}");
            }
            if modi != Modifier::None {
                assert_eq!(got.quality.modifier, modi, "{title}");
            }
        }
    }
}
