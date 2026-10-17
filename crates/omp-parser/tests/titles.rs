//! Movie title parsing cases extracted from Radarr's ParserFixture,
//! EditionParserFixture, CrapParserFixture and HashedReleaseFixture.

mod common;

use common::{cases, check_all, s};
use omp_parser::title::{normalize_imdb_id, parse_movie_path, parse_movie_title};
use serde_json::Value;

fn quality_by_name(name: &str) -> omp_core::quality::Quality {
    use omp_core::quality::Quality as Q;
    match name {
        "WEBDL720p" => Q::WEBDL_720P,
        "WEBDL1080p" => Q::WEBDL_1080P,
        "WEBDL2160p" => Q::WEBDL_2160P,
        "Bluray720p" => Q::BLURAY_720P,
        "Bluray1080p" => Q::BLURAY_1080P,
        "HDTV720p" => Q::HDTV_720P,
        "HDTV1080p" => Q::HDTV_1080P,
        "DVD" => Q::DVD,
        "SDTV" => Q::SDTV,
        other => panic!("add {other} to quality_by_name"),
    }
}

fn eq<T: PartialEq + std::fmt::Debug>(got: T, want: T) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err(format!("got {got:?}, want {want:?}"))
    }
}

#[test]
fn movie_title() {
    check_all(
        "should_parse_movie_title",
        cases("ParserFixture", "should_parse_movie_title"),
        |a| {
            let p = parse_movie_title(s(&a[0]), false).ok_or("did not parse")?;
            eq(p.primary_title().unwrap_or_default(), s(&a[1]))
        },
    );
}

#[test]
fn anime_movie_title() {
    check_all(
        "anime with year",
        cases("ParserFixture", "should_parse_anime_movie_title"),
        |a| {
            let p = parse_movie_title(s(&a[0]), false).ok_or("did not parse")?;
            eq(p.primary_title().unwrap_or_default(), s(&a[1]))?;
            eq(p.release_group.as_deref(), Some(s(&a[2])))?;
            eq(i64::from(p.year), a[3].as_i64().unwrap())
        },
    );
}

#[test]
fn anime_movie_title_without_year() {
    check_all(
        "anime without year",
        cases(
            "ParserFixture",
            "should_parse_anime_movie_title_without_year",
        ),
        |a| {
            let p = parse_movie_title(s(&a[0]), false).ok_or("did not parse")?;
            eq(p.primary_title().unwrap_or_default(), s(&a[1]))?;
            eq(p.release_group.as_deref(), Some(s(&a[2])))
        },
    );
}

#[test]
fn german_movies() {
    check_all(
        "german",
        cases("ParserFixture", "should_parse_german_movie"),
        |a| {
            let p = parse_movie_title(s(&a[0]), false).ok_or("did not parse")?;
            eq(p.primary_title().unwrap_or_default(), s(&a[1]))?;
            eq(p.edition.as_str(), s(&a[2]))?;
            eq(i64::from(p.year), a[3].as_i64().unwrap())
        },
    );
}

#[test]
fn alternative_titles() {
    check_all(
        "aka",
        cases("ParserFixture", "should_parse_movie_alternative_titles"),
        |a| {
            let p = parse_movie_title(s(&a[0]), true).ok_or("did not parse")?;
            let want: Vec<&str> = a[1].as_array().unwrap().iter().map(s).collect();
            let got: Vec<&str> = p.movie_titles.iter().map(String::as_str).collect();
            eq(got, want)
        },
    );
}

#[test]
fn folder_name() {
    check_all(
        "folder",
        cases("ParserFixture", "should_parse_movie_folder_name"),
        |a| {
            let p = parse_movie_title(s(&a[0]), true).ok_or("did not parse")?;
            eq(p.primary_title().unwrap_or_default(), s(&a[1]))
        },
    );
}

#[test]
fn year_tmdb_imdb_subs() {
    check_all(
        "year",
        cases("ParserFixture", "should_parse_movie_year"),
        |a| {
            let p = parse_movie_title(s(&a[0]), false).ok_or("did not parse")?;
            eq(i64::from(p.year), a[1].as_i64().unwrap())
        },
    );
    check_all(
        "tmdb",
        cases("ParserFixture", "should_parse_tmdb_id"),
        |a| {
            let p = parse_movie_title(s(&a[0]), false).ok_or("did not parse")?;
            eq(i64::from(p.tmdb_id), a[1].as_i64().unwrap())
        },
    );
    check_all(
        "hardcoded subs",
        cases("ParserFixture", "should_parse_hardcoded_subs"),
        |a| {
            let p = parse_movie_title(s(&a[0]), false).ok_or("did not parse")?;
            eq(p.hardcoded_subs.as_deref(), a[1].as_str())
        },
    );
    check_all(
        "imdb",
        cases("ParserFixture", "should_parse_imdb_in_title"),
        |a| {
            let p = parse_movie_title(s(&a[0]), true).ok_or("did not parse")?;
            eq(p.imdb_id.as_str(), s(&a[1]))
        },
    );
    check_all(
        "normalize imdb",
        cases("ParserFixture", "should_normalize_imdbid"),
        |a| {
            eq(
                normalize_imdb_id(s(&a[0])),
                a[1].as_str().map(str::to_owned),
            )
        },
    );
}

#[test]
fn editions() {
    check_all(
        "edition",
        cases("EditionParserFixture", "should_parse_edition"),
        |a| {
            let p = parse_movie_title(s(&a[0]), false).ok_or("did not parse")?;
            eq(p.edition.as_str(), s(&a[1]))
        },
    );
    check_all(
        "no edition",
        cases("EditionParserFixture", "should_not_parse_edition"),
        |a| {
            let p = parse_movie_title(s(&a[0]), false).ok_or("did not parse")?;
            eq(p.edition.as_str(), "")
        },
    );
}

#[test]
fn crap_is_rejected() {
    for method in [
        "should_not_parse_crap",
        "should_not_parse_file_name_without_proper_spacing",
        "should_not_parse_invalid_release_name",
    ] {
        check_all(
            method,
            cases("CrapParserFixture", method),
            |a| match parse_movie_title(s(&a[0]), false) {
                None => Ok(()),
                Some(p) => Err(format!("parsed as {p}")),
            },
        );
    }
}

#[test]
fn random_hashes_are_rejected() {
    // Radarr runs 500 random 32/40-char strings; use a fixed pseudo-random sequence.
    let charset: Vec<char> = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
        .chars()
        .collect();
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    for length in [32, 40] {
        for _ in 0..500 {
            let hash: String = (0..length)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    charset[(state % charset.len() as u64) as usize]
                })
                .collect();
            assert!(parse_movie_title(&hash, false).is_none(), "parsed {hash}");
        }
    }
}

#[test]
fn hashed_releases_via_path() {
    let path = format!(
        "{}/tests/data/hashed_release_cases.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let all: Vec<Vec<Value>> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(all.len() >= 8, "only {} cases", all.len());
    check_all("hashed", all, |a| {
        // The fixtures use Windows paths; ours handles both separators.
        let p = parse_movie_path(s(&a[0])).ok_or("did not parse")?;
        eq(p.primary_title().unwrap_or_default(), s(&a[1]))?;
        eq(p.release_group.as_deref(), Some(s(&a[3])))?;
        let want = quality_by_name(s(&a[2]));
        eq(p.quality.quality, want)
    });
}
