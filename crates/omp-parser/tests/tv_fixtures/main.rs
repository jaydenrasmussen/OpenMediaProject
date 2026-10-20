//! Sonarr's own parser test cases, run against the port. The tables are
//! generated from Sonarr's NUnit fixtures; see scripts/gen_tv_fixtures.py.

use chrono::{Datelike, Duration, Utc};
use omp_parser::tv::{
    ParsedEpisodeInfo, clean_series_title, normalize_episode_title, parse_path, parse_season_title,
    parse_series_name, parse_title,
};

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum Arg {
    S(&'static str),
    I(i64),
    F(f64),
    B(bool),
    L(&'static [i32]),
    N,
}

#[allow(unused_imports)]
use Arg::{B, F, I, L, N, S};

include!("data.rs");

impl Arg {
    fn s(self) -> &'static str {
        match self {
            S(s) => s,
            other => panic!("not a string: {other:?}"),
        }
    }
    fn i(self) -> i32 {
        match self {
            I(i) => i as i32,
            other => panic!("not an int: {other:?}"),
        }
    }
    fn l(self) -> &'static [i32] {
        match self {
            L(l) => l,
            other => panic!("not a list: {other:?}"),
        }
    }
}

/// Runs every case, collecting what went wrong so one run shows all of it.
fn run(cases: &[&[Arg]], check: impl Fn(&[Arg], Option<ParsedEpisodeInfo>) -> Result<(), String>) {
    let mut failures = Vec::new();
    for c in cases {
        let title = c[0].s();
        let parsed = parse_title(title);
        if let Err(why) = check(c, parsed) {
            failures.push(format!("{title:?}: {why}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} failed:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

fn need(p: Option<ParsedEpisodeInfo>) -> Result<ParsedEpisodeInfo, String> {
    p.ok_or_else(|| "did not parse".to_owned())
}

fn eq<T: PartialEq + std::fmt::Debug>(what: &str, got: T, want: T) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err(format!("{what}: got {got:?}, want {want:?}"))
    }
}

#[test]
fn single_episodes() {
    for table in [
        SINGLE_SHOULD_PARSE_SINGLE_EPISODE,
        SINGLE_SHOULD_PARSE_JAPANESE_VARIETY_SHOW_FORMAT,
    ] {
        run(table, |c, p| {
            let p = need(p)?;
            eq("episodes", p.episode_numbers.clone(), vec![c[3].i()])?;
            eq("season", p.season_number(), Some(c[2].i()))?;
            eq("series", p.series_title.as_str(), c[1].s())?;
            eq("absolute", p.absolute_episode_numbers.is_empty(), true)?;
            eq("full season", p.full_season, false)
        });
    }
}

#[test]
fn decimal_numbers_are_specials() {
    run(
        SINGLE_SPECIAL_SHOULD_PARSE_DECIMAL_NUMBER_AS_SPECIAL,
        |c, p| {
            let p = need(p)?;
            eq("episodes", p.episode_numbers.clone(), vec![c[3].i()])?;
            eq("season", p.season_number(), Some(c[2].i()))?;
            eq("series", p.series_title.as_str(), c[1].s())?;
            eq("special", p.special, true)
        },
    );
}

#[test]
fn split_episodes() {
    run(SINGLE_SPLIT_SHOULD_PARSE_SPLIT_EPISODE, |c, p| {
        let p = need(p)?;
        eq("episodes", p.episode_numbers.clone(), vec![c[3].i()])?;
        eq("season", p.season_number(), Some(c[2].i()))?;
        eq("series", p.series_title.as_str(), c[1].s())?;
        eq("split", p.is_split_episode, true)
    });
}

#[test]
fn multi_episode_releases() {
    run(MULTI_SHOULD_PARSE_MULTIPLE_EPISODES, |c, p| {
        let p = need(p)?;
        eq("season", p.season_number(), Some(c[2].i()))?;
        eq("episodes", p.episode_numbers.clone(), c[3].l().to_vec())?;
        eq("series", p.series_title.as_str(), c[1].s())?;
        eq("absolute", p.absolute_episode_numbers.is_empty(), true)?;
        eq("full season", p.full_season, false)
    });
}

#[test]
fn full_seasons() {
    run(SEASON_SHOULD_PARSE_FULL_SEASON_RELEASE, |c, p| {
        let p = need(p)?;
        eq("season", p.season_number(), Some(c[2].i()))?;
        eq("series", p.series_title.as_str(), c[1].s())?;
        eq("episodes", p.episode_numbers.is_empty(), true)?;
        eq("full season", p.full_season, true)
    });
}

#[test]
fn season_extras_and_subpacks() {
    for table in [
        SEASON_EXTRAS_SHOULD_PARSE_SEASON_EXTRAS,
        SEASON_EXTRAS_SHOULD_PARSE_SEASON_SUBPACK,
    ] {
        run(table, |c, p| {
            let p = need(p)?;
            eq("season", p.season_number(), Some(c[2].i()))?;
            eq("series", p.series_title.as_str(), c[1].s())?;
            eq("full season", p.full_season, true)?;
            eq("extra", p.is_season_extra, true)
        });
    }
}

#[test]
fn partial_seasons() {
    run(SEASON_PART_SHOULD_PARSE_PARTIAL_SEASON_RELEASE, |c, p| {
        let p = need(p)?;
        eq("season", p.season_number(), Some(c[2].i()))?;
        eq("series", p.series_title.as_str(), c[1].s())?;
        eq("partial", p.is_partial_season, true)?;
        eq("full season", p.full_season, false)?;
        eq("part", p.season_part, c[3].i())
    });
}

#[test]
fn multi_season_packs() {
    run(MULTI_SEASON_SHOULD_PARSE_MULTI_SEASON_RELEASE, |c, p| {
        let p = need(p)?;
        eq("series", p.series_title.as_str(), c[1].s())?;
        eq("seasons", p.season_numbers.clone(), c[2].l().to_vec())?;
        eq("full season", p.full_season, true)?;
        eq("partial", p.is_partial_season, false)
    });
}

fn air_date(c: &[Arg]) -> String {
    format!("{:04}-{:02}-{:02}", c[2].i(), c[3].i(), c[4].i())
}

#[test]
fn daily_episodes() {
    for table in [
        DAILY_SHOULD_PARSE_DAILY_EPISODE,
        DAILY_SHOULD_PARSE_DAILY_EPISODE_USING_SHORT_MONTH_FORMAT,
    ] {
        run(table, |c, p| {
            let p = need(p)?;
            eq("series", p.series_title.as_str(), c[1].s())?;
            eq("air date", p.air_date.clone(), Some(air_date(c)))?;
            eq("episodes", p.episode_numbers.is_empty(), true)?;
            eq("full season", p.full_season, false)
        });
    }
    run(
        DAILY_PART_SHOULD_PARSE_DAILY_EPISODE_WITH_MULTIPLE_PARTS,
        |c, p| {
            let p = need(p)?;
            eq("series", p.series_title.as_str(), c[1].s())?;
            eq("air date", p.air_date.clone(), Some(air_date(c)))?;
            eq("part", p.daily_part, Some(c[5].i()))
        },
    );
}

#[test]
fn ambiguous_daily_dates_are_refused() {
    run(
        AMBIGUOUS_SHOULD_NOT_PARSE_AMBIGUOUS_DAILY_EPISODE,
        |_, p| eq("parsed", p.is_none(), true),
    );
}

#[test]
fn dates_in_the_future() {
    let at = |days: i64| {
        let d = Utc::now().date_naive() + Duration::days(days);
        format!(
            "A Late Talk Show {:04} {:02} {:02} 720p HDTV x264-GRP",
            d.year(),
            d.month(),
            d.day()
        )
    };
    assert!(
        parse_title(&at(2)).is_some(),
        "a couple of days early is fine"
    );
    assert!(
        parse_title(&at(20)).is_none(),
        "weeks ahead is a parse error"
    );
    assert!(
        parse_title("A Late Talk Show 1950 10 14 720p HDTV x264-GRP").is_none(),
        "ancient dates are refused"
    );
}

#[test]
fn absolute_numbering() {
    run(ABSOLUTE_SHOULD_PARSE_ABSOLUTE_NUMBERS, |c, p| {
        let p = need(p)?;
        eq(
            "absolute",
            p.absolute_episode_numbers.clone(),
            vec![c[2].i()],
        )?;
        eq("season", p.season_number(), Some(c[3].i()))?;
        eq(
            "episodes",
            p.episode_numbers.first().copied().unwrap_or(0),
            c[4].i(),
        )?;
        eq("series", p.series_title.as_str(), c[1].s())?;
        eq("full season", p.full_season, false)
    });
}

#[test]
fn absolute_specials() {
    run(ABSOLUTE_SPECIAL_SHOULD_PARSE_ABSOLUTE_SPECIALS, |c, p| {
        let p = need(p)?;
        eq(
            "absolute",
            p.absolute_episode_numbers.clone(),
            vec![c[2].i()],
        )?;
        eq("season", p.season_number(), Some(0))?;
        eq("series", p.series_title.as_str(), c[1].s())?;
        eq("special", p.special, true)
    });
    run(
        ABSOLUTE_SPECIAL_BARE_SHOULD_PARSE_ABSOLUTE_SPECIALS_WITHOUT_ABSOLUTE_NUMBER,
        |c, p| {
            let p = need(p)?;
            eq("absolute", p.absolute_episode_numbers.is_empty(), true)?;
            eq("season", p.season_number(), Some(0))?;
            eq("series", p.series_title.as_str(), c[1].s())?;
            eq("special", p.special, true)
        },
    );
}

#[test]
fn absolute_ranges() {
    run(
        ABSOLUTE_RANGE_SHOULD_PARSE_MULTI_EPISODE_ABSOLUTE_NUMBERS,
        |c, p| {
            let p = need(p)?;
            let want: Vec<i32> = (c[2].i()..=c[3].i()).collect();
            eq("absolute", p.absolute_episode_numbers.clone(), want)?;
            eq("series", p.series_title.as_str(), c[1].s())?;
            eq("full season", p.full_season, false)
        },
    );
}

#[test]
fn anime_season_packs() {
    run(ANIME_SEASON_SHOULD_PARSE_ANIME_SEASON_PACKS, |c, p| {
        let p = need(p)?;
        eq("absolute", p.absolute_episode_numbers.is_empty(), true)?;
        eq("series", p.series_title.as_str(), c[1].s())?;
        eq("full season", p.full_season, true)?;
        eq("season", p.season_number(), Some(c[2].i()))
    });
}

#[test]
fn mini_series() {
    for table in [
        MINI_SHOULD_PARSE_MINI_SERIES_EPISODE,
        MINI_SHOULD_PARSE_KOREAN_SERIES_EPISODE,
    ] {
        run(table, |c, p| {
            let p = need(p)?;
            eq("episodes", p.episode_numbers.clone(), vec![c[2].i()])?;
            eq("season", p.season_number(), Some(1))?;
            eq("series", p.series_title.as_str(), c[1].s())?;
            eq("full season", p.full_season, false)
        });
    }
}

#[test]
fn crap_is_not_parsed() {
    run(CRAP_SHOULD_NOT_PARSE_CRAP, |_, p| {
        eq("parsed", p.is_none(), true)
    });
}

#[test]
fn series_names_compare_clean() {
    run(SERIES_NAME_SHOULD_PARSE_SERIES_NAME, |c, _| {
        eq(
            "name",
            clean_series_title(&parse_series_name(c[0].s())),
            clean_series_title(c[1].s()),
        )
    });
}

#[test]
fn series_title_info_splits_off_the_year() {
    run(TITLE_INFO_SHOULD_PARSE_SERIES_TITLE_INFO, |c, p| {
        let info = need(p)?.series_title_info;
        eq("without year", info.title_without_year.as_str(), c[1].s())?;
        eq("year", info.year, c.get(2).map_or(0, |y| y.i()))
    });
}

#[test]
fn request_info_is_removed_from_titles() {
    run(
        REQUEST_INFO_SHOULD_REMOVE_REQUEST_INFO_FROM_TITLE,
        |c, p| eq("series", need(p)?.series_title.as_str(), c[1].s()),
    );
}

#[test]
fn several_titles_in_one_release() {
    run(ALL_TITLES_SHOULD_PARSE_MULTIPLE_SERIES_TITLES, |c, p| {
        let mut got = need(p)?.series_title_info.all_titles;
        let mut want: Vec<String> = c[1..].iter().map(|a| a.s().to_owned()).collect();
        got.sort();
        want.sort();
        eq("titles", got, want)
    });
}

#[test]
fn titles_are_cleaned_for_matching() {
    run(CLEAN_SHOULD_NORMALIZE_SERIES_TITLE, |c, _| {
        eq("clean", clean_series_title(c[0].s()), c[1].s().to_owned())
    });
    run(CLEAN_SHOULD_REMOVE_SPECIAL_CHARACTERS_AND_CASING, |c, _| {
        eq("clean", clean_series_title(c[0].s()), c[1].s().to_owned())
    });
}

#[test]
fn episode_titles_are_normalized() {
    run(EPISODE_TITLE_SHOULD_NORMALIZE_EPISODE_TITLE, |c, _| {
        eq(
            "title",
            normalize_episode_title(c[0].s()),
            c[1].s().to_owned(),
        )
    });
}

#[test]
fn paths_parse_from_folder_and_file() {
    run(PATH_SHOULD_PARSE_FROM_PATH, |c, _| {
        let p = need(parse_path(c[0].s()))?;
        eq("episodes", p.episode_numbers.clone(), vec![c[2].i()])?;
        eq("season", p.season_number(), Some(c[1].i()))?;
        eq("absolute", p.absolute_episode_numbers.is_empty(), true)?;
        eq("full season", p.full_season, false)
    });
    run(PATH_MULTI_SHOULD_PARSE_MULTI_EPISODE_FROM_PATH, |c, _| {
        let p = need(parse_path(c[0].s()))?;
        eq("series", p.series_title.as_str(), c[1].s())?;
        eq("season", p.season_number(), Some(c[2].i()))?;
        eq("episodes", p.episode_numbers.clone(), c[3].l().to_vec())
    });
}

#[test]
fn anime_releases_with_unicode_groups() {
    run(
        GROUP_ABS_SHOULD_PARSE_CHINESE_ANIME_RELEASES,
        check_group_abs,
    );
    run(
        GROUP_ABS_SHOULD_PARSE_CHINESE_ANIME_SEASON_EPISODE_RELEASES,
        check_group_abs,
    );
    run(
        GROUP_ABS_SHOULD_PARSE_UNBRACKETED_CHINESE_ANIME_RELEASES,
        check_group_abs,
    );
    run(
        GROUP_ABS_SHOULD_PARSE_FALSE_POSITIVE_CHINESE_ANIME_RELEASES,
        check_group_abs,
    );
}

fn check_group_abs(c: &[Arg], p: Option<ParsedEpisodeInfo>) -> Result<(), String> {
    let p = need(p)?;
    eq("group", p.release_group.clone(), Some(c[2].s().to_owned()))?;
    eq(
        "absolute",
        p.absolute_episode_numbers.clone(),
        vec![c[3].i()],
    )?;
    eq("series", p.series_title.as_str(), c[1].s())?;
    eq("full season", p.full_season, false)
}

#[test]
fn chinese_multi_episode_releases() {
    run(
        GROUP_ABS_RANGE_SHOULD_PARSE_CHINESE_MULTIEPISODE_RELEASES,
        |c, p| {
            let p = need(p)?;
            eq("group", p.release_group.clone(), Some(c[2].s().to_owned()))?;
            eq(
                "absolute",
                p.absolute_episode_numbers.clone(),
                c[3].l().to_vec(),
            )?;
            eq("series", p.series_title.as_str(), c[1].s())
        },
    );
}

#[test]
fn gm_team_and_unicode_digits() {
    run(GM_TEAM_SHOULD_PARSE_GM_TEAM_RELEASES_AND_FILES, |c, p| {
        let p = need(p)?;
        eq(
            "absolute",
            p.absolute_episode_numbers.clone(),
            vec![c[2].i()],
        )?;
        eq("series", p.series_title.as_str(), c[1].s())?;
        eq("group", p.release_group.as_deref(), Some("GM-Team"))
    });
    run(UNICODE_DIGITS_SHOULD_PARSE_UNICODE_DIGITS, |c, p| {
        let p = need(p)?;
        eq(
            "absolute",
            p.absolute_episode_numbers.clone(),
            vec![c[2].i()],
        )?;
        eq("series", p.series_title.as_str(), c[1].s())
    });
}

#[test]
fn chinese_season_packs() {
    run(UNICODE_SEASON_SHOULD_PARSE_FULL_SEASON_RELEASES, |c, p| {
        let p = need(p)?;
        eq("group", p.release_group.clone(), Some(c[2].s().to_owned()))?;
        eq("series", p.series_title.as_str(), c[1].s())?;
        eq("season", p.season_number(), Some(c[3].i()))?;
        eq("full season", p.full_season, true)
    });
    run(UNICODE_MULTI_SHOULD_PARSE_MULTI_EPISODE_RELEASE, |c, p| {
        let p = need(p)?;
        eq("season", p.season_number(), Some(c[3].i()))?;
        eq("episodes", p.episode_numbers.clone(), c[4].l().to_vec())?;
        eq("series", p.series_title.as_str(), c[1].s())
    });
}

#[test]
fn sub_groups_and_hashes() {
    run(GROUP_HASH_SHOULD_PARSE_RELEASEGROUP_AND_HASH, |c, p| {
        let p = need(p)?;
        eq("group", p.release_group.as_deref(), Some(c[1].s()))?;
        eq("hash", p.release_hash.as_str(), c[2].s())
    });
    run(HASH_SHOULD_PARSE_RELEASE_HASHES_WITH_10B_OR_8B, |c, p| {
        eq("hash", need(p)?.release_hash.as_str(), c[1].s())
    });
}

#[test]
fn season_titles_without_numbers() {
    run(
        SEASON_TITLE_SHOULD_RETURN_THE_TITLE_WHEN_THERE_IS_NO_SEASON_OR_EPISODE,
        |c, _| {
            let p = need(parse_season_title(c[0].s()))?;
            eq("series", p.series_title.as_str(), c[1].s())?;
            eq("full season", p.full_season, true)?;
            eq("season title", p.is_season_title, true)?;
            eq("season", p.season_number(), None)
        },
    );
    for table in [
        SEASON_TITLE_NONE_SHOULD_NOT_RETURN_A_TITLE_WHEN_STRIPPING_LEAVES_METADATA,
        SEASON_TITLE_NONE_SHOULD_NOT_RETURN_A_TITLE_FOR_AN_EXTRA,
    ] {
        run(table, |c, _| {
            eq("parsed", parse_season_title(c[0].s()).is_none(), true)
        });
    }
}
