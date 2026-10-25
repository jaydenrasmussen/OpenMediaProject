//! Episode title parsing. Ported from Sonarr `Parser.ParseTitle` (GPL-3.0).
//!
//! The patterns are Sonarr's own, generated into `tv_patterns.rs` and run on
//! [`crate::netre`], which keeps the per-iteration captures the original
//! relies on (multi-episode ranges, multi-season packs). Patterns are tried
//! in order and the first that yields a result wins, so their order matters.

use std::sync::LazyLock;

use chrono::{Datelike, Duration, NaiveDate, Utc};
use omp_core::{language::Language, quality::QualityModel};
use unicode_normalization::UnicodeNormalization;

use crate::{
    files::remove_file_extension,
    language::parse_languages,
    netre::{Match, Regex},
    normalize::remove_accent,
    quality::{parse_quality, parse_quality_name},
    release_group::parse_release_group,
    tv_patterns as p,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeriesTitleInfo {
    pub title: String,
    pub title_without_year: String,
    pub year: i32,
    pub all_titles: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseType {
    Unknown,
    SingleEpisode,
    MultiEpisode,
    SeasonPack,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedEpisodeInfo {
    pub release_title: String,
    pub series_title: String,
    pub series_title_info: SeriesTitleInfo,
    pub quality: QualityModel,
    pub season_numbers: Vec<i32>,
    pub episode_numbers: Vec<i32>,
    pub absolute_episode_numbers: Vec<i32>,
    pub special_absolute_episode_numbers: Vec<f64>,
    /// `yyyy-MM-dd` for daily shows.
    pub air_date: Option<String>,
    pub languages: Vec<Language>,
    pub full_season: bool,
    pub is_partial_season: bool,
    pub is_season_extra: bool,
    pub is_season_title: bool,
    pub is_split_episode: bool,
    pub is_mini_series: bool,
    pub special: bool,
    pub release_group: Option<String>,
    pub release_hash: String,
    pub season_part: i32,
    pub release_tokens: String,
    pub daily_part: Option<i32>,
}

impl ParsedEpisodeInfo {
    /// The first season named, if any.
    pub fn season_number(&self) -> Option<i32> {
        self.season_numbers.first().copied()
    }

    pub fn is_multi_season(&self) -> bool {
        self.season_numbers.len() > 1
    }

    pub fn is_daily(&self) -> bool {
        self.air_date
            .as_deref()
            .is_some_and(|d| !d.trim().is_empty())
    }

    pub fn is_absolute_numbering(&self) -> bool {
        !self.absolute_episode_numbers.is_empty()
    }

    pub fn is_possible_special_episode(&self) -> bool {
        let no_air_date = self.air_date.as_deref().is_none_or(|d| d.trim().is_empty());
        let no_title = self.series_title.trim().is_empty();
        (no_air_date
            && no_title
            && (self.episode_numbers.is_empty() || self.season_number() == Some(0)))
            || (!no_title && self.special)
            || (self.episode_numbers.len() == 1 && self.episode_numbers[0] == 0)
    }

    pub fn is_possible_scene_season_special(&self) -> bool {
        self.season_number().is_some_and(|s| s != 0)
            && self.episode_numbers.len() == 1
            && self.episode_numbers[0] == 0
    }

    pub fn release_type(&self) -> ReleaseType {
        if self.episode_numbers.len() > 1 || self.absolute_episode_numbers.len() > 1 {
            ReleaseType::MultiEpisode
        } else if self.episode_numbers.len() == 1 || self.absolute_episode_numbers.len() == 1 {
            ReleaseType::SingleEpisode
        } else if self.full_season {
            ReleaseType::SeasonPack
        } else {
            ReleaseType::Unknown
        }
    }
}

/// A date in the title that cannot be real; parsing stops (the title is not
/// retried against later patterns).
#[derive(Debug)]
struct InvalidDate;

fn compile((pattern, ignore_case): (&str, bool)) -> Regex {
    let built = if ignore_case {
        Regex::ci(pattern)
    } else {
        Regex::new(pattern)
    };
    built.unwrap_or_else(|e| panic!("bad built-in pattern {pattern}: {e}"))
}

static REPORT_TITLE: LazyLock<Vec<Regex>> =
    LazyLock::new(|| p::REPORT_TITLE.iter().map(|x| compile(*x)).collect());
static SPECIAL_EPISODE_TITLE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    p::SPECIAL_EPISODE_TITLE
        .iter()
        .map(|x| compile(*x))
        .collect()
});
static REJECT_HASHED: LazyLock<Vec<Regex>> =
    LazyLock::new(|| p::REJECT_HASHED.iter().map(|x| compile(*x)).collect());
static SEASON_FOLDERS: LazyLock<Vec<Regex>> =
    LazyLock::new(|| p::SEASON_FOLDERS.iter().map(|x| compile(*x)).collect());

struct Substitution {
    regex: Regex,
    replacement: &'static str,
}

fn substitutions(list: &'static [(&'static str, &'static str, bool)]) -> Vec<Substitution> {
    list.iter()
        .map(|(pattern, replacement, ci)| Substitution {
            regex: compile((pattern, *ci)),
            replacement,
        })
        .collect()
}

static PRE_SUBSTITUTION: LazyLock<Vec<Substitution>> =
    LazyLock::new(|| substitutions(p::PRE_SUBSTITUTION));
static PATH_PRE_SUBSTITUTION: LazyLock<Vec<Substitution>> =
    LazyLock::new(|| substitutions(p::PATH_PRE_SUBSTITUTION));

macro_rules! single {
    ($name:ident, $konst:ident) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| compile(p::$konst));
    };
}

single!(REVERSED_TITLE, REVERSED_TITLE);
single!(ANIME_SEASON_RELEASE_METADATA, ANIME_SEASON_RELEASE_METADATA);
single!(ANIME_SPECIAL_MARKER, ANIME_SPECIAL_MARKER);
single!(NORMALIZE, NORMALIZE);
single!(PERCENT, PERCENT);
single!(SIMPLE_TITLE, SIMPLE_TITLE);
single!(SIX_DIGIT_AIR_DATE, SIX_DIGIT_AIR_DATE);
single!(CLEAN_QUALITY_BRACKETS, CLEAN_QUALITY_BRACKETS);
single!(YEAR_IN_TITLE, YEAR_IN_TITLE);
single!(TITLE_COMPONENTS, TITLE_COMPONENTS);
single!(PART, PART);
single!(PUNCTUATION, PUNCTUATION);
single!(ARTICLE_WORD, ARTICLE_WORD);
single!(DUPLICATE_SPACES, DUPLICATE_SPACES);
single!(SEASON_FOLDER, SEASON_FOLDER);
single!(SIMPLE_EPISODE_NUMBER, SIMPLE_EPISODE_NUMBER);
single!(REQUEST_INFO, REQUEST_INFO);
single!(MULTI, MULTI);
single!(WEBSITE_PREFIX, WEBSITE_PREFIX);
single!(WEBSITE_POSTFIX, WEBSITE_POSTFIX);
single!(CLEAN_TORRENT_SUFFIX, CLEAN_TORRENT_SUFFIX);

const NUMBER_WORDS: [&str; 10] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
];

const SHORT_MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// Digits in any script become ASCII (Sonarr normalizes with NFKC first).
fn to_numerals(value: &str) -> String {
    value
        .nfkc()
        .map(|c| match c.to_digit(10) {
            Some(d) => char::from_digit(d, 10).unwrap_or(c),
            None => c,
        })
        .collect()
}

fn parse_number(value: &str) -> Option<i32> {
    to_numerals(value).trim().parse().ok().or_else(|| {
        NUMBER_WORDS
            .iter()
            .position(|w| *w == value.to_lowercase())
            .map(|i| i as i32)
    })
}

fn parse_decimal(value: &str) -> Option<f64> {
    to_numerals(value).trim().parse().ok()
}

/// Series names the engine should treat as already clean.
fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// Lower-case, accent-free title with punctuation and filler words removed,
/// for comparing titles (Sonarr `CleanSeriesTitle`).
pub fn clean_series_title(title: &str) -> String {
    if title.trim().is_empty() || all_digits(title) {
        return title.to_owned();
    }
    let title = PERCENT.replace_all(title, "percent");
    remove_diacritics(&NORMALIZE.replace_all_template(&title, "").to_lowercase())
}

/// Accents removed, plus the letters that do not decompose (ð, ø, ł, ...),
/// which Sonarr's diacritics library also flattens.
fn remove_diacritics(text: &str) -> String {
    remove_accent(text)
        .chars()
        .flat_map(|c| -> Vec<char> {
            match c {
                'ð' | 'đ' => vec!['d'],
                'Ð' | 'Đ' => vec!['D'],
                'ø' => vec!['o'],
                'Ø' => vec!['O'],
                'ł' => vec!['l'],
                'Ł' => vec!['L'],
                'ı' => vec!['i'],
                'þ' => vec!['t', 'h'],
                'Þ' => vec!['T', 'h'],
                'ß' => vec!['s', 's'],
                'æ' => vec!['a', 'e'],
                'Æ' => vec!['A', 'E'],
                'œ' => vec!['o', 'e'],
                'Œ' => vec!['O', 'E'],
                other => vec![other],
            }
        })
        .collect()
}

/// An episode title reduced to lower-case words (Sonarr `NormalizeEpisodeTitle`).
pub fn normalize_episode_title(title: &str) -> String {
    let mut title = title.to_owned();
    for re in SPECIAL_EPISODE_TITLE.iter() {
        if let Some(m) = re.find(&title) {
            if let Some(t) = m.group("episodetitle") {
                title = t.to_owned();
            }
            break;
        }
    }
    let title = PART.replace_all(&title, "");
    let title = PUNCTUATION.replace_all(&title, " ");
    let title = DUPLICATE_SPACES.replace_all(&title, " ");
    title.trim().to_lowercase()
}

/// A title for loose comparison (Sonarr `NormalizeTitle`).
pub fn normalize_title(title: &str) -> String {
    let title = PUNCTUATION.replace_all(title, "");
    let title = ARTICLE_WORD.replace_all(&title, "");
    let title = DUPLICATE_SPACES.replace_all(&title, " ");
    title.trim().to_lowercase()
}

pub fn has_multiple_languages(title: &str) -> bool {
    MULTI.is_match(title)
}

fn validate_before_parsing(title: &str) -> bool {
    let lower = title.to_lowercase();
    if lower.contains("password") && lower.contains("yenc") {
        return false;
    }
    if !title.chars().any(char::is_alphanumeric) {
        return false;
    }
    let bare = remove_file_extension(title);
    if REJECT_HASHED.iter().any(|r| r.is_match(&bare)) {
        return false;
    }
    !SEASON_FOLDERS.iter().any(|r| r.is_match(&bare))
}

fn unreverse(title: &str) -> String {
    if !REVERSED_TITLE.is_match(title) {
        return title.to_owned();
    }
    let bare = remove_file_extension(title);
    let reversed: String = bare.chars().rev().collect();
    format!("{reversed}{}", &title[bare.len()..])
}

/// Strips site prefixes/suffixes without otherwise changing the title.
pub fn simplify_title(title: &str) -> String {
    if !validate_before_parsing(title) {
        return title.to_owned();
    }
    let title = unreverse(title);
    let simple = WEBSITE_PREFIX.replace_all(&title, "");
    let simple = WEBSITE_POSTFIX.replace_all(&simple, "");
    CLEAN_TORRENT_SUFFIX.replace_all(&simple, "")
}

fn series_title_info(title: &str, matches: &[Match]) -> SeriesTitleInfo {
    let mut info = SeriesTitleInfo {
        title: title.to_owned(),
        ..Default::default()
    };
    match YEAR_IN_TITLE.find(title) {
        None => info.title_without_year = title.to_owned(),
        Some(m) => {
            info.title_without_year = m.group("title").unwrap_or_default().to_owned();
            info.year = m.group("year").and_then(|y| y.parse().ok()).unwrap_or(0);
        }
    }
    if let Some(components) = TITLE_COMPONENTS.find(&info.title_without_year) {
        info.all_titles = components
            .captures("title")
            .into_iter()
            .map(str::to_owned)
            .collect();
    } else {
        let titles = matches[0].captures("title");
        if titles.len() > 1 {
            info.all_titles = titles
                .into_iter()
                .map(|t| t.replace(['.', '_'], " "))
                .collect();
        }
    }
    info
}

fn parse_match_collection(
    matches: &[Match],
    release_title: &str,
) -> Result<Option<ParsedEpisodeInfo>, InvalidDate> {
    let first = &matches[0];
    let series_name = first
        .group("title")
        .unwrap_or_default()
        .replace(['.', '_'], " ");
    let series_name = REQUEST_INFO
        .replace_all(&series_name, "")
        .trim_matches(' ')
        .to_owned();
    let air_year: i32 = first
        .group("airyear")
        .and_then(|y| y.parse().ok())
        .unwrap_or(0);
    let mut last_index = first.group_end("title").unwrap_or(0);

    let mut result;
    if air_year < 1900 {
        result = ParsedEpisodeInfo {
            release_title: release_title.to_owned(),
            season_numbers: vec![0],
            ..Default::default()
        };
        for m in matches {
            let episodes = m.capture_ranges("episode");
            let absolutes = m.capture_ranges("absoluteepisode");
            let text = |r: &std::ops::Range<usize>| &release_title_of(m)[r.clone()];

            // Zero episodes is fine: it means a whole season.
            if let (Some(f), Some(l)) = (episodes.first(), episodes.last()) {
                let (Some(lo), Some(hi)) = (parse_number(text(f)), parse_number(text(l))) else {
                    return Ok(None);
                };
                if lo > hi {
                    return Ok(None);
                }
                result.episode_numbers = (lo..=hi).collect();
                last_index = last_index.max(l.end);
                if m.success("special") {
                    result.special = true;
                }
                if m.success("splitepisode") {
                    result.is_split_episode = true;
                }
            }

            if let (Some(f), Some(l)) = (absolutes.first(), absolutes.last()) {
                let (Some(lo), Some(hi)) = (parse_decimal(text(f)), parse_decimal(text(l))) else {
                    return Ok(None);
                };
                if lo > hi {
                    return Ok(None);
                }
                if lo.fract() != 0.0 || hi.fract() != 0.0 {
                    if absolutes.len() != 1 {
                        return Ok(None); // several matches are not allowed for specials
                    }
                    result.special_absolute_episode_numbers = vec![lo];
                    result.special = true;
                    last_index = last_index.max(f.end);
                } else {
                    result.absolute_episode_numbers = (lo as i32..=hi as i32).collect();
                    if m.success("special") {
                        result.special = true;
                    }
                    last_index = last_index.max(l.end);
                }
            }

            if episodes.is_empty() && absolutes.is_empty() {
                // "Extras" and "SUBPACK" releases are flagged so they can be filtered out.
                if first.group("extras").is_some_and(|e| !e.trim().is_empty()) {
                    result.is_season_extra = true;
                }
                let part = first.group("seasonpart").unwrap_or_default();
                if !part.trim().is_empty() {
                    result.season_part = part.parse().unwrap_or(0);
                    result.is_partial_season = true;
                } else if first.success("special") {
                    result.special = true;
                } else {
                    result.full_season = true;
                }
            }

            if episodes.len() == 2
                && first.success("episodecount")
                && first.group("episodecount") == Some(text(&episodes[1]))
            {
                result.episode_numbers.clear();
                result.full_season = true;
            }
        }

        let mut seasons: Vec<i32> = Vec::new();
        for range in first.capture_ranges("season") {
            if let Ok(s) = release_title_of(first)[range.clone()].parse::<i32>() {
                seasons.push(s);
                last_index = last_index.max(range.end);
            }
        }
        seasons.sort_unstable();
        seasons.dedup();
        match seasons.len() {
            1 | 3.. => result.season_numbers = seasons,
            2 => result.season_numbers = (seasons[0]..=seasons[1]).collect(),
            _ => {
                if result.absolute_episode_numbers.is_empty() && !result.episode_numbers.is_empty()
                {
                    // No season and not absolute-only: a mini series, season 1.
                    result.season_numbers = vec![1];
                    result.is_mini_series = true;
                }
            }
        }
    } else {
        // A daily show.
        let (mut month, mut day);
        if first.success("ambiguousairmonth") && first.success("ambiguousairday") {
            let m: i32 = first
                .group("ambiguousairmonth")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let d: i32 = first
                .group("ambiguousairday")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            if d <= 12 && m <= 12 {
                return Err(InvalidDate);
            }
            month = m;
            day = d;
        } else if first.success("shortairmonth") {
            let short = first
                .group("shortairmonth")
                .unwrap_or_default()
                .to_lowercase();
            let Some(index) = SHORT_MONTHS.iter().position(|m| *m == short) else {
                return Err(InvalidDate);
            };
            month = index as i32 + 1;
            day = first
                .group("airday")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
        } else {
            month = first
                .group("airmonth")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            day = first
                .group("airday")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
        }
        // Scene releases sometimes swap day and month.
        if month > 12 {
            std::mem::swap(&mut month, &mut day);
        }
        let date = NaiveDate::from_ymd_opt(air_year, month.max(0) as u32, day.max(0) as u32)
            .ok_or(InvalidDate)?;
        // Early releases are fine; dates far in the future are parse errors.
        if date > Utc::now().date_naive() + Duration::days(14) {
            return Err(InvalidDate);
        }
        let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).expect("valid date");
        if date < epoch && first.group("titleyear").is_none_or(|y| y.trim().is_empty()) {
            return Err(InvalidDate);
        }
        for name in ["airyear", "airmonth", "airday"] {
            last_index = last_index.max(first.group_end(name).unwrap_or(0));
        }
        result = ParsedEpisodeInfo {
            release_title: release_title.to_owned(),
            season_numbers: vec![0],
            air_date: Some(format!(
                "{:04}-{:02}-{:02}",
                date.year(),
                date.month(),
                date.day()
            )),
            ..Default::default()
        };
        if let Some(part) = first.group("part") {
            result.daily_part = part.parse().ok();
        }
    }

    result.release_tokens =
        if last_index < release_title.len() && release_title.is_char_boundary(last_index) {
            release_title[last_index..].to_owned()
        } else {
            release_title.to_owned()
        };
    result.series_title = series_name;
    result.series_title_info = series_title_info(&result.series_title, matches);
    Ok(Some(result))
}

/// The text a match was made on. Capture ranges are offsets into it.
fn release_title_of<'t>(m: &Match<'t>) -> &'t str {
    m.source()
}

fn get_release_hash(m: &Match) -> String {
    let Some(hash) = m.group("hash") else {
        return String::new();
    };
    let value = hash.trim_matches(['[', ']', '(', ')']);
    if value == "1280x720" {
        String::new()
    } else {
        value.to_owned()
    }
}

/// Parses a release title or file name. `None` when it does not look like an
/// episode release.
pub fn parse_title(title: &str) -> Option<ParsedEpisodeInfo> {
    if !validate_before_parsing(title) {
        return None;
    }
    let title_owned = unreverse(title);
    let title = title_owned.as_str();

    let mut release_title = remove_file_extension(title)
        .replace('【', "[")
        .replace('】', "]");
    for s in PRE_SUBSTITUTION.iter() {
        if s.regex.is_match(&release_title) {
            release_title = s.regex.replace_all_template(&release_title, s.replacement);
        }
    }

    let mut simple = SIMPLE_TITLE.replace_all(&release_title, "");
    simple = WEBSITE_PREFIX.replace_all(&simple, "");
    simple = WEBSITE_POSTFIX.replace_all(&simple, "");
    simple = CLEAN_TORRENT_SUFFIX.replace_all(&simple, "");
    // A trailing "[WEB-DL]" style tag is quality noise, not title.
    if let Some(m) = CLEAN_QUALITY_BRACKETS.find(&simple)
        && parse_quality_name(m.as_str()).quality != omp_core::quality::Quality::UNKNOWN
    {
        simple = simple[..m.range().start].to_owned();
    }

    if let Some(m) = SIX_DIGIT_AIR_DATE.find(&simple) {
        let (year, month, day) = (
            m.group("airyear").unwrap_or_default(),
            m.group("airmonth").unwrap_or_default(),
            m.group("airday").unwrap_or_default(),
        );
        if month != "00" || day != "00" {
            let fixed = format!("20{year}.{month}.{day}");
            simple = simple.replace(m.group("airdate").unwrap_or_default(), &fixed);
        }
    }

    for regex in REPORT_TITLE.iter() {
        let matches = regex.find_all(&simple);
        if matches.is_empty() {
            continue;
        }
        let parsed = match parse_match_collection(&matches, &release_title) {
            Ok(r) => r,
            Err(InvalidDate) => break,
        };
        let Some(mut result) = parsed else { continue };

        if result.full_season && result.release_tokens.to_lowercase().contains("special") {
            result.full_season = false;
            result.special = true;
        }
        result.languages = parse_languages(&result.release_tokens);
        result.quality = parse_quality(title);
        result.release_group = parse_release_group(&release_title);
        if let Some(sub) = matches[0]
            .group("subgroup")
            .filter(|s| !s.trim().is_empty())
        {
            result.release_group = Some(sub.to_owned());
        }
        result.release_hash = get_release_hash(&matches[0]);
        return Some(result);
    }
    None
}

/// Parses a season pack named after the season's own title ("Show Name
/// Season Title [Group]"), which has no numbers to find.
pub fn parse_season_title(title: &str) -> Option<ParsedEpisodeInfo> {
    let release_title = remove_file_extension(title);
    if ANIME_SPECIAL_MARKER.is_match(&release_title) {
        return None;
    }
    let series = ANIME_SEASON_RELEASE_METADATA.replace_all(&release_title, " ");
    let series = DUPLICATE_SPACES.replace_all(&series, " ");
    let series = series.trim().to_owned();
    if series.is_empty() || series.contains(['[', ']', '(', ')', '{', '}']) {
        return None;
    }
    Some(ParsedEpisodeInfo {
        release_title: release_title.clone(),
        series_title: series.clone(),
        series_title_info: SeriesTitleInfo {
            title: series.clone(),
            title_without_year: series,
            ..Default::default()
        },
        full_season: true,
        is_season_title: true,
        languages: parse_languages(&release_title),
        quality: parse_quality(title),
        release_group: parse_release_group(&release_title),
        ..Default::default()
    })
}

/// The series name in a title, or the cleaned title when it is not an episode.
pub fn parse_series_name(title: &str) -> String {
    parse_title(title).map_or_else(|| clean_series_title(title), |p| p.series_title)
}

fn file_parts(path: &str) -> (String, Option<String>, String) {
    let normalized = path.replace('\\', "/");
    let mut parts: Vec<&str> = normalized.split('/').collect();
    let file = parts.pop().unwrap_or_default().to_owned();
    let dir = parts.pop().filter(|d| !d.is_empty()).map(str::to_owned);
    let extension = crate::files::path_extension(&file).to_owned();
    (file, dir, extension)
}

/// Parses a file path, using its folder when the file name alone is not enough.
pub fn parse_path(path: &str) -> Option<ParsedEpisodeInfo> {
    let (mut file_name, dir, extension) = file_parts(path);
    for s in PATH_PRE_SUBSTITUTION.iter() {
        if s.regex.is_match(&file_name) {
            file_name = s.regex.replace_all_template(&file_name, s.replacement);
        }
    }
    let mut result = parse_title(&file_name);

    // The folder says the season, the file says the episode: combine them.
    if let (Some(number), Some(dir_name)) = (SIMPLE_EPISODE_NUMBER.find(&file_name), &dir)
        && result
            .as_ref()
            .is_none_or(|r| r.is_mini_series || !r.absolute_episode_numbers.is_empty())
        && let Some(season) = SEASON_FOLDER.find(dir_name)
        && let Some(season_text) = season.group("season")
    {
        let episodes = number.captures("episode");
        let lo = parse_number(episodes.first()?)?;
        let hi = parse_number(episodes.last()?)?;
        let mut path_title = format!("S{season_text}E{lo:02}");
        if lo != hi {
            path_title.push_str(&format!("-E{hi:02}"));
        }
        if let Some(rest) = number.group("remaining") {
            path_title.push_str(&format!(" {rest}"));
        }
        return parse_title(&path_title);
    }

    if result.is_none()
        && let Some(number) = {
            let stem = file_name.strip_suffix(&extension).unwrap_or(&file_name);
            stem.parse::<i32>().ok()
        }
        && let Some(dir_name) = &dir
    {
        result = parse_title(dir_name);
        result = match result {
            Some(mut r) if r.absolute_episode_numbers.contains(&number) => {
                r.absolute_episode_numbers = vec![number];
                Some(r)
            }
            Some(mut r) if r.episode_numbers.contains(&number) => {
                r.episode_numbers = vec![number];
                Some(r)
            }
            _ => None,
        };
    }

    if result.is_none()
        && let Some(dir_name) = &dir
    {
        result = parse_title(&format!("{dir_name} {file_name}"));
        if result.is_none() {
            result = parse_title(&format!("{dir_name}{extension}"));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_pattern_compiles() {
        assert!(REPORT_TITLE.len() > 100);
        assert!(PRE_SUBSTITUTION.len() >= 10);
        let _ = (&*SIMPLE_TITLE, &*NORMALIZE, &*SIX_DIGIT_AIR_DATE, &*MULTI);
        let _ = (&*PATH_PRE_SUBSTITUTION, &*SPECIAL_EPISODE_TITLE);
    }

    fn episode(title: &str) -> (String, Option<i32>, Vec<i32>) {
        let p = parse_title(title).unwrap_or_else(|| panic!("{title} did not parse"));
        (
            p.series_title.clone(),
            p.season_number(),
            p.episode_numbers.clone(),
        )
    }

    #[test]
    fn single_episodes() {
        assert_eq!(
            episode("The.Mentalist.S03E09.720p.HDTV.x264-IMMERSE"),
            ("The Mentalist".into(), Some(3), vec![9])
        );
        assert_eq!(
            episode("Castle.2009.S01E14.English.HDTV.XviD-LOL"),
            ("Castle 2009".into(), Some(1), vec![14])
        );
        assert_eq!(
            episode("Two.and.a.Half.Men.S12E01.1080p.WEB-DL.DD5.1.H.264-NTb"),
            ("Two and a Half Men".into(), Some(12), vec![1])
        );
    }

    #[test]
    fn multi_episode_ranges_use_every_capture() {
        assert_eq!(
            episode("Fargo.S02E01E02.720p.HDTV.x264-KILLERS"),
            ("Fargo".into(), Some(2), vec![1, 2])
        );
        assert_eq!(
            episode("Show.Name.S01E05-06.720p.HDTV-GRP"),
            ("Show Name".into(), Some(1), vec![5, 6])
        );
        assert_eq!(
            episode("Show.Name.S01E01E02E03.720p.HDTV-GRP").2,
            vec![1, 2, 3]
        );
    }

    #[test]
    fn season_packs_and_multi_season_packs() {
        let p = parse_title("Game.of.Thrones.S01.1080p.BluRay.x264-DEMAND").unwrap();
        assert!(p.full_season);
        assert_eq!(
            (p.series_title.as_str(), p.season_numbers),
            ("Game of Thrones", vec![1])
        );

        let p = parse_title("The.Wire.S01-S05.720p.BluRay.x264-GRP").unwrap();
        assert!(p.full_season && p.is_multi_season());
        assert_eq!(p.season_numbers, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn daily_shows() {
        let p = parse_title("The.Daily.Show.2018.04.28.720p.HDTV.x264-GRP").unwrap();
        assert!(p.is_daily());
        assert_eq!(p.air_date.as_deref(), Some("2018-04-28"));
        assert_eq!(p.series_title, "The Daily Show");
        // Ambiguous day/month is refused rather than guessed.
        assert!(parse_title("Show.04.05.2018.720p.HDTV-GRP").is_none());
        assert_eq!(
            parse_title("Show.28.04.2018.720p.HDTV-GRP")
                .unwrap()
                .air_date
                .as_deref(),
            Some("2018-04-28")
        );
    }

    #[test]
    fn anime_absolute_numbering_and_groups() {
        let p = parse_title("[HorribleSubs] Naruto Shippuuden - 347 [720p]").unwrap();
        assert_eq!(p.series_title, "Naruto Shippuuden");
        assert_eq!(p.absolute_episode_numbers, vec![347]);
        assert_eq!(p.release_group.as_deref(), Some("HorribleSubs"));
        assert!(p.is_absolute_numbering());
    }

    #[test]
    fn junk_is_rejected() {
        for bad in [
            "d2d6a3c4f8e44cb9a1b2c3d4e5f60718",
            "Season 1",
            "!!!",
            "Backup_12345S01-02",
        ] {
            assert!(parse_title(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn titles_clean_up_for_comparison() {
        assert_eq!(clean_series_title("The Walking Dead"), "thewalkingdead");
        assert_eq!(
            clean_series_title("Fear the Walking Dead"),
            "fearwalkingdead"
        );
        assert_eq!(
            clean_series_title("Marvel's Agents of S.H.I.E.L.D."),
            "marvelsagentsshield"
        );
        assert_eq!(clean_series_title("3%"), "3percent");
        assert_eq!(clean_series_title("1234"), "1234");
        assert_eq!(normalize_title("The Office (US)"), "office us");
        assert_eq!(normalize_episode_title("Pilot (1)"), "pilot");
    }

    #[test]
    fn paths_combine_folder_and_file() {
        let p = parse_path("/tv/Show Name/Season 02/05 - Title.mkv").unwrap();
        assert_eq!(p.season_number(), Some(2));
        assert_eq!(p.episode_numbers, vec![5]);
        let p = parse_path("/tv/Show/Show.S01E02.720p.HDTV-GRP.mkv").unwrap();
        assert_eq!(p.episode_numbers, vec![2]);
    }
}
