//! File and folder names for episodes. Ported from Sonarr `FileNameBuilder`
//! (GPL-3.0); the pattern language is the same so existing formats keep working.

use std::{collections::HashMap, sync::LazyLock};

use omp_parser::re::Re;

use crate::{
    builder::{
        Context, MultiEpisodeStyle, NamingConfig, NamingFile, NamingMovie, TRIM_TRAILING,
        TokenMatch, finish_ellipsis, replace_reserved_device_names, simplify, split_pattern,
        truncate,
    },
    text::{
        clean_file_name, clean_folder_name, clean_title, clean_title_the, collapse_separators,
        title_first_character, title_the,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NamingSeriesType {
    #[default]
    Standard,
    Daily,
    Anime,
}

#[derive(Debug, Clone, Default)]
pub struct NamingSeries {
    pub title: String,
    pub year: i32,
    pub imdb_id: Option<String>,
    pub tvdb_id: Option<i64>,
    pub tmdb_id: i64,
    pub series_type: NamingSeriesType,
}

#[derive(Debug, Clone, Default)]
pub struct NamingEpisode {
    pub season: i32,
    pub episode: i32,
    pub absolute: Option<i32>,
    pub title: String,
    /// `YYYY-MM-DD`.
    pub air_date: Option<String>,
}

static TOKEN: LazyLock<Re> = LazyLock::new(|| {
    Re::ci(
        r"(?<escaped>\{\{|\}\})|\{(?<prefix>[- ._\[(]*)(?<token>(?:[a-z0-9]+)(?:(?<separator>[- ._]+)(?:[a-z0-9]+))?)(?::(?<customFormat>[ ,a-z0-9+-]+(?<![- ])))?(?<suffix>[- ._)\]]*)\}",
    )
});
static SEASON_EPISODE: LazyLock<Re> = LazyLock::new(|| {
    Re::ci(
        r"(?<sepBefore>(?<=\})[- ._]+?)?(?<seasonEpisode>s?\{season(?:\:0+)?\}(?<episodeSeparator>[- ._]?[ex])(?<episode>\{episode(?:\:0+)?\}))(?<sepAfter>[- ._]+?(?=\{))?",
    )
});
static ABSOLUTE: LazyLock<Re> = LazyLock::new(|| {
    Re::ci(
        r"(?<sepBefore>(?<=\})[- ._]+?)?(?<absolute>\{absolute(?:\:0+)?\})(?<sepAfter>[- ._]+?(?=\{))?",
    )
});
static EPISODE_NUM: LazyLock<Re> = LazyLock::new(|| Re::ci(r"(?<episode>\{episode(?:\:0+)?\})"));
static SEASON_NUM: LazyLock<Re> = LazyLock::new(|| Re::ci(r"(?<season>\{season(?:\:0+)?\})"));
static ABSOLUTE_NUM: LazyLock<Re> = LazyLock::new(|| Re::ci(r"(?<absolute>\{absolute(?:\:0+)?\})"));
static AIR_DATE: LazyLock<Re> = LazyLock::new(|| Re::ci(r"\{Air(\s|\W|_)Date\}"));
static MULTI_PART: LazyLock<Re> =
    LazyLock::new(|| Re::ci(r"(?:\:?\s?(?:\(\d+\)|(Part|Pt\.?)\s?\d+))$"));
static YEAR_SUFFIX: LazyLock<Re> = LazyLock::new(|| Re::ci(r"\(\d{4}\)$"));

const MAX_NAME_BYTES: usize = 255;

/// `ToString(format)` for a number: a run of zeros pads to that width.
fn number(value: i32, format: Option<&str>) -> String {
    match format {
        Some(f) if !f.is_empty() && f.chars().all(|c| c == '0') => {
            format!("{value:0width$}", width = f.len())
        }
        _ => value.to_string(),
    }
}

/// Fills a `{episode:00}`-style token with a value.
fn replace_number_token(token: &str, value: i32) -> String {
    let inner = token.trim_matches(['{', '}']);
    number(value, inner.split_once(':').map(|(_, f)| f))
}

fn title_year(title: &str, year: i32) -> String {
    if year == 0 || YEAR_SUFFIX.is_match(title) {
        title.to_owned()
    } else {
        format!("{title} ({year})")
    }
}

fn title_without_year(title: &str) -> String {
    YEAR_SUFFIX.replace_all(title, "")
}

fn clean_title_the_year(title: &str, year: i32) -> String {
    if year == 0 {
        return clean_title_the(title);
    }
    if let Some(m) = YEAR_SUFFIX.captures(title) {
        let head = &title[..m.whole().start];
        let y = &m.whole_str()[1..5];
        return format!("{} {y}", clean_title_the(head.trim()));
    }
    format!("{} {year}", clean_title_the(title))
}

fn series_value(
    series: &NamingSeries,
    title: &str,
    token: &str,
    custom: Option<&str>,
) -> Option<String> {
    let t = |s: String| Some(truncate(&s, custom));
    match token {
        "seriestitle" => t(title.to_owned()),
        "seriescleantitle" => t(clean_title(title)),
        "seriestitleyear" => t(title_year(title, series.year)),
        "seriescleantitleyear" => t(clean_title(&title_year(title, series.year))),
        "seriestitlewithoutyear" => t(title_without_year(title)),
        "seriescleantitlewithoutyear" => t(clean_title(&title_without_year(title))),
        "seriestitlethe" => t(title_the(title)),
        "seriescleantitlethe" => t(clean_title_the(title)),
        "seriestitletheyear" => t(title_year(&title_the(title), series.year)),
        "seriescleantitletheyear" => t(clean_title_the_year(title, series.year)),
        "seriestitlethewithoutyear" => t(title_without_year(&title_the(title))),
        "seriescleantitlethewithoutyear" => t(clean_title_the(&title_without_year(title))),
        "seriestitlefirstcharacter" => t(title_first_character(&title_the(title))),
        "seriesyear" => Some(series.year.to_string()),
        "imdbid" => Some(series.imdb_id.clone().unwrap_or_default()),
        "tvdbid" => Some(series.tvdb_id.map(|i| i.to_string()).unwrap_or_default()),
        "tmdbid" => Some(if series.tmdb_id > 0 {
            series.tmdb_id.to_string()
        } else {
            String::new()
        }),
        _ => None,
    }
}

fn episode_titles(episodes: &[NamingEpisode]) -> Vec<String> {
    let trim = |s: &str| s.trim_end_matches([' ', '.', '?']).to_owned();
    if episodes.len() == 1 {
        return vec![trim(&episodes[0].title)];
    }
    let mut titles: Vec<String> = Vec::new();
    for e in episodes {
        let t = MULTI_PART
            .replace_all(&trim(&e.title), "")
            .trim()
            .to_owned();
        if !titles.contains(&t) {
            titles.push(t);
        }
    }
    if titles.iter().all(|t| t.trim().is_empty()) {
        titles.clear();
        for e in episodes {
            let t = trim(&e.title);
            if !titles.contains(&t) {
                titles.push(t);
            }
        }
    }
    titles
}

fn cut_bytes(s: &str, max: usize) -> String {
    let mut end = max.min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

fn join_titles(
    titles: &[String],
    separator: &str,
    max_length: usize,
    formatter: Option<&str>,
) -> String {
    let formatter_max: usize = formatter.and_then(|f| f.trim().parse().ok()).unwrap_or(0);
    let max_length = if formatter_max > 0 {
        max_length.min(formatter_max)
    } else {
        max_length
    };
    let joined = titles.join(&format!(" {} ", separator.trim()));
    if joined.len() <= max_length {
        return joined;
    }
    let Some(first) = titles.first() else {
        return String::new();
    };
    let trim = |s: &str| s.trim_end_matches([' ', '.']).to_owned();
    if titles.len() >= 2 {
        let last = titles.last().unwrap();
        if first.len() + last.len() + 3 <= max_length {
            return format!("{}{{ellipsis}}{last}", trim(first));
        }
    }
    if titles.len() > 1 && first.len() + 3 <= max_length {
        return format!("{}{{ellipsis}}", trim(first));
    }
    if titles.len() == 1 && first.len() <= max_length {
        return first.clone();
    }
    format!(
        "{}{{ellipsis}}",
        trim(&cut_bytes(first, max_length.saturating_sub(3)))
    )
}

type Handler<'a> = Box<dyn Fn(&TokenMatch<'_>) -> Option<String> + 'a>;
type Fallback<'a> = Box<dyn Fn(&str, &TokenMatch<'_>) -> Option<String> + 'a>;

struct Handlers<'a> {
    map: HashMap<String, Handler<'a>>,
    fallback: Fallback<'a>,
}

impl<'a> Handlers<'a> {
    fn set(&mut self, name: &str, f: impl Fn(&TokenMatch<'_>) -> Option<String> + 'a) {
        self.map.insert(simplify(name), Box::new(f));
    }

    fn value(&self, token: &str, m: &TokenMatch<'_>) -> Option<String> {
        match self.map.get(token) {
            Some(f) => f(m),
            None => (self.fallback)(token, m),
        }
    }
}

fn replace_tokens(pattern: &str, h: &Handlers<'_>, config: &NamingConfig, escape: bool) -> String {
    TOKEN.replace_with(pattern, |c| {
        if c.has("escaped") {
            return if escape {
                c.whole_str().to_owned()
            } else if c.whole_str() == "{{" {
                "{".into()
            } else {
                "}".into()
            };
        }
        let token = c.name("token").unwrap_or_default();
        let separator = c.name("separator").unwrap_or_default();
        let m = TokenMatch {
            prefix: c.name("prefix").unwrap_or_default(),
            suffix: c.name("suffix").unwrap_or_default(),
            custom: c.name("customFormat").filter(|s| !s.trim().is_empty()),
        };
        let Some(text) = h.value(&simplify(token), &m) else {
            // The handler wants a second pass (the episode title).
            return c.whole_str().to_owned();
        };
        let mut text = text.trim().to_owned();
        if token
            .chars()
            .all(|t| !t.is_alphabetic() || t.is_lowercase())
        {
            text = text.to_lowercase();
        } else if token
            .chars()
            .all(|t| !t.is_alphabetic() || t.is_uppercase())
        {
            text = text.to_uppercase();
        }
        if !separator.trim().is_empty() {
            text = text.replace(' ', separator);
        }
        let mut text = clean_file_name(&text, config);
        if !text.trim().is_empty() {
            text = format!("{}{text}{}", m.prefix, m.suffix);
        }
        if escape {
            text = text.replace('{', "{{").replace('}', "}}");
        }
        text
    })
}

struct EpisodeFormat {
    separator: String,
    episode_separator: String,
    episode_pattern: String,
    season_episode_pattern: String,
}

fn episode_formats(pattern: &str) -> Vec<EpisodeFormat> {
    let mut out: Vec<EpisodeFormat> = Vec::new();
    for c in SEASON_EPISODE.captures_all(pattern) {
        let f = EpisodeFormat {
            separator: c
                .name("sepAfter")
                .or(c.name("sepBefore"))
                .unwrap_or_default()
                .to_owned(),
            episode_separator: c.name("episodeSeparator").unwrap_or_default().to_owned(),
            episode_pattern: c.name("episode").unwrap_or_default().to_owned(),
            season_episode_pattern: c.name("seasonEpisode").unwrap_or_default().to_owned(),
        };
        if !out
            .iter()
            .any(|o| o.season_episode_pattern == f.season_episode_pattern)
        {
            out.push(f);
        }
    }
    out
}

fn format_numbers(base: &str, repeat: &str, episodes: &[&NamingEpisode]) -> String {
    let mut out = String::new();
    for (i, e) in episodes.iter().enumerate() {
        let source = if i == 0 { base } else { repeat };
        out += &EPISODE_NUM.replace_with(source, |c| {
            replace_number_token(c.name("episode").unwrap_or_default(), e.episode)
        });
    }
    SEASON_NUM.replace_with(&out, |c| {
        replace_number_token(c.name("season").unwrap_or_default(), episodes[0].season)
    })
}

fn first_and_last(episodes: &[NamingEpisode]) -> Vec<&NamingEpisode> {
    let mut v = vec![&episodes[0]];
    if episodes.len() > 1 {
        v.push(&episodes[episodes.len() - 1]);
    }
    v
}

/// Rewrites the season/episode part of the pattern for however many
/// episodes the file holds, returning the new pattern and the `{Season
/// EpisodeN}` values to fill in.
fn season_episode_tokens(
    mut pattern: String,
    episodes: &[NamingEpisode],
    style: MultiEpisodeStyle,
    values: &mut Vec<(String, String)>,
) -> String {
    let all: Vec<&NamingEpisode> = episodes.iter().collect();
    for (i, f) in episode_formats(&pattern).iter().enumerate() {
        let (repeat, set) = match style {
            MultiEpisodeStyle::Duplicate => {
                (format!("{}{}", f.separator, f.season_episode_pattern), &all)
            }
            MultiEpisodeStyle::Repeat => (
                format!("{}{}", f.episode_separator, f.episode_pattern),
                &all,
            ),
            MultiEpisodeStyle::Scene => (
                format!("-{}{}", f.episode_separator, f.episode_pattern),
                &all,
            ),
            MultiEpisodeStyle::Range => {
                (format!("-{}", f.episode_pattern), &first_and_last(episodes))
            }
            MultiEpisodeStyle::PrefixedRange => (
                format!("-{}{}", f.episode_separator, f.episode_pattern),
                &first_and_last(episodes),
            ),
            MultiEpisodeStyle::Extend => (format!("-{}", f.episode_pattern), &all),
        };
        let filled = format_numbers(&f.season_episode_pattern, &repeat, set);
        let token = format!("{{Season Episode{}}}", i + 1);
        pattern = pattern.replace(&f.season_episode_pattern, &token);
        values.push((token, filled));
    }
    pattern
}

fn absolute_tokens(
    mut pattern: String,
    series: &NamingSeries,
    episodes: &[NamingEpisode],
    style: MultiEpisodeStyle,
    values: &mut Vec<(String, String)>,
) -> String {
    let mut formats: Vec<(String, String)> = Vec::new();
    for c in ABSOLUTE.captures_all(&pattern) {
        let sep = c
            .name("sepAfter")
            .or(c.name("sepBefore"))
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("-");
        let abs = c.name("absolute").unwrap_or_default().to_owned();
        if !formats.iter().any(|(_, a)| *a == abs) {
            formats.push((sep.to_owned(), abs));
        }
    }
    for (i, (sep, abs)) in formats.iter().enumerate() {
        if series.series_type != NamingSeriesType::Anime
            || episodes.iter().any(|e| e.absolute.is_none())
        {
            pattern = pattern.replace(abs, "");
            continue;
        }
        let eps: Vec<&NamingEpisode> = match style {
            MultiEpisodeStyle::Range | MultiEpisodeStyle::PrefixedRange => first_and_last(episodes),
            _ => episodes.iter().collect(),
        };
        let repeat = match style {
            MultiEpisodeStyle::Duplicate => format!("{sep}{abs}"),
            MultiEpisodeStyle::Repeat => {
                let t = sep.trim();
                format!("{}{abs}", if t.is_empty() { " " } else { t })
            }
            _ => format!("-{abs}"),
        };
        let mut filled = String::new();
        for (n, e) in eps.iter().enumerate() {
            let source = if n == 0 {
                abs.as_str()
            } else {
                repeat.as_str()
            };
            filled += &ABSOLUTE_NUM.replace_with(source, |c| {
                replace_number_token(
                    c.name("absolute").unwrap_or_default(),
                    e.absolute.unwrap_or(0),
                )
            });
        }
        let filled = SEASON_NUM.replace_with(&filled, |c| {
            replace_number_token(c.name("season").unwrap_or_default(), episodes[0].season)
        });
        let token = format!("{{Absolute Pattern{}}}", i + 1);
        pattern = pattern.replace(abs, &token);
        values.push((token, filled));
    }
    pattern
}

fn pattern_has_identifier(pattern: &str) -> bool {
    SEASON_EPISODE.is_match(pattern) || ABSOLUTE.is_match(pattern) || AIR_DATE.is_match(pattern)
}

/// The tokens that need no episode: series, ids and the file.
fn base_handlers<'a>(
    series: &'a NamingSeries,
    title: &'a str,
    file: Option<&'a NamingFile>,
    multiple: bool,
    anime: bool,
) -> Handlers<'a> {
    Handlers {
        map: HashMap::new(),
        fallback: Box::new(move |token, m| {
            if let Some(v) = series_value(series, title, token, m.custom) {
                return Some(v);
            }
            let file = file?;
            let dummy = NamingMovie::default();
            let ctx = Context {
                movie: &dummy,
                file: Some(file),
                multiple_tokens: multiple,
            };
            // Anime shows a revision as "v2"; everything else says "Proper".
            let quality_title = file.quality_title.clone();
            Some(match token {
                "qualityfull" => quality_title
                    .map(|t| format!("{t} {} {}", proper(file, anime), real(file)))
                    .unwrap_or_default(),
                "qualityproper" => proper(file, anime),
                "releasehash" => file.release_hash.clone().unwrap_or_default(),
                _ => ctx.file_value(file, token, m),
            })
        }),
    }
}

fn proper(file: &NamingFile, anime: bool) -> String {
    if file.revision_version > 1 {
        if anime {
            format!("v{}", file.revision_version)
        } else {
            "Proper".into()
        }
    } else {
        String::new()
    }
}

fn real(file: &NamingFile) -> String {
    if file.revision_real > 0 {
        "REAL".into()
    } else {
        String::new()
    }
}

/// The file name (no extension) for a file holding `episodes`, or a relative
/// path when the pattern has folder separators. `extension_bytes` reserves
/// room under the 255 byte file name limit.
pub fn build_episode_file_name(
    series: &NamingSeries,
    episodes: &[NamingEpisode],
    file: &NamingFile,
    extension_bytes: usize,
    config: &NamingConfig,
) -> Result<String, String> {
    if episodes.is_empty() {
        return Err("No episodes to name".into());
    }
    let mut episodes = episodes.to_vec();
    episodes.sort_by_key(|e| (e.season, e.episode));
    if !config.rename_episodes {
        return Ok(crate::builder::original_title_of(file));
    }
    let pattern = match series.series_type {
        NamingSeriesType::Daily if episodes[0].season > 0 => &config.daily_episode_format,
        NamingSeriesType::Anime
            if episodes.iter().all(|e| e.absolute.is_some())
                || !ABSOLUTE.is_match(&config.anime_episode_format) =>
        {
            &config.anime_episode_format
        }
        _ => &config.standard_episode_format,
    };
    if pattern.trim().is_empty() {
        return Err("Episode format cannot be empty".into());
    }
    let anime = series.series_type == NamingSeriesType::Anime;
    let parts: Vec<&str> = split_pattern(pattern).collect();
    let mut components = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let has_id = pattern_has_identifier(part);
        let mut values = Vec::new();
        let mut part = season_episode_tokens(
            (*part).to_owned(),
            &episodes,
            config.multi_episode_style,
            &mut values,
        );
        part = absolute_tokens(
            part,
            series,
            &episodes,
            config.multi_episode_style,
            &mut values,
        );
        part = part.replace("...", "{{ellipsis}}");
        // {Original Filename} is only useful when nothing else names the episode.
        let multiple = has_id && file.relative_path.is_some();
        let mut h = base_handlers(series, &series.title, Some(file), multiple, anime);
        for (token, value) in values {
            h.set(&token, move |_| Some(value.clone()));
        }
        let first = episodes[0].clone();
        let last = episodes[episodes.len() - 1].clone();
        let many = episodes.len() > 1;
        h.set("Season", move |m| Some(number(first.season, m.custom)));
        {
            let (f, l) = (episodes[0].clone(), last.clone());
            h.set("Episode", move |m| {
                Some(if many {
                    format!(
                        "{}-{}",
                        number(f.episode, m.custom),
                        number(l.episode, m.custom)
                    )
                } else {
                    number(f.episode, m.custom)
                })
            });
        }
        let air = episodes[0].air_date.clone();
        h.set("Air Date", move |_| {
            Some(match air.as_deref().filter(|a| !a.trim().is_empty()) {
                Some(a) => a.replace('-', " "),
                None => "Unknown".into(),
            })
        });
        // The episode title is filled in on the second pass, once its room is known.
        h.set("Episode Title", |_| None);
        h.set("Episode CleanTitle", |_| None);
        let mut component = replace_tokens(&part, &h, config, true).trim().to_owned();
        let mut max_segment = MAX_NAME_BYTES;
        if i == parts.len() - 1 {
            max_segment = max_segment.saturating_sub(extension_bytes);
        }
        let without_title = {
            let mut h2 = base_handlers(series, &series.title, Some(file), multiple, anime);
            h2.set("Episode Title", |_| Some(String::new()));
            h2.set("Episode CleanTitle", |_| Some(String::new()));
            h2.set("ellipsis", |_| Some("...".into()));
            replace_tokens(&component, &h2, config, false).len()
        };
        let max_title = max_segment.saturating_sub(without_title);
        let titles = episode_titles(&episodes);
        let clean: Vec<String> = titles.iter().map(|t| clean_title(t)).collect();
        h.set("Episode Title", {
            let titles = titles.clone();
            move |m| Some(join_titles(&titles, "+", max_title, m.custom))
        });
        h.set("Episode CleanTitle", move |m| {
            Some(join_titles(&clean, "and", max_title, m.custom))
        });
        component = replace_tokens(&component, &h, config, false)
            .trim()
            .to_owned();

        let component = collapse_separators(&component);
        let component = TRIM_TRAILING.replace_all(&component, "");
        let component = replace_reserved_device_names(&finish_ellipsis(&component));
        components.push(component);
    }
    Ok(components.join("/"))
}

pub fn build_series_folder(series: &NamingSeries, config: &NamingConfig) -> String {
    let h = base_handlers(series, &series.title, None, false, false);
    let name = clean_folder_name(&replace_tokens(
        &config.series_folder_format,
        &h,
        config,
        false,
    ));
    finish_ellipsis(&replace_reserved_device_names(&name))
}

pub fn build_season_folder(
    series: &NamingSeries,
    season: i32,
    season_title: Option<&str>,
    config: &NamingConfig,
) -> String {
    let mut h = base_handlers(series, &series.title, None, false, false);
    h.set("Season", move |m| Some(number(season, m.custom)));
    let st = season_title.unwrap_or_default().to_owned();
    let st2 = st.clone();
    h.set("Season Title", move |m| {
        Some(if st.trim().is_empty() {
            String::new()
        } else {
            truncate(&st, m.custom)
        })
    });
    h.set("Season CleanTitle", move |m| {
        Some(if st2.trim().is_empty() {
            String::new()
        } else {
            truncate(&clean_title(&st2), m.custom)
        })
    });
    let format = if season == 0 {
        &config.specials_folder_format
    } else {
        &config.season_folder_format
    };
    let name = clean_folder_name(&replace_tokens(format, &h, config, false));
    let name = name.trim_matches([' ', '.', '-', '_']).to_owned();
    finish_ellipsis(&replace_reserved_device_names(&name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series() -> NamingSeries {
        NamingSeries {
            title: "Some Show".into(),
            year: 2020,
            tvdb_id: Some(7),
            ..Default::default()
        }
    }

    fn ep(season: i32, episode: i32, title: &str) -> NamingEpisode {
        NamingEpisode {
            season,
            episode,
            absolute: None,
            title: title.into(),
            air_date: Some("2021-03-04".into()),
        }
    }

    fn file() -> NamingFile {
        NamingFile {
            quality_title: Some("WEBDL-1080p".into()),
            release_group: Some("GRP".into()),
            scene_name: Some("Some.Show.S01E02.1080p.WEB-DL-GRP".into()),
            ..Default::default()
        }
    }

    fn cfg() -> NamingConfig {
        NamingConfig {
            rename_episodes: true,
            ..Default::default()
        }
    }

    fn name(eps: &[NamingEpisode], config: &NamingConfig) -> String {
        build_episode_file_name(&series(), eps, &file(), 4, config).unwrap()
    }

    #[test]
    fn default_pattern_names_a_single_episode() {
        assert_eq!(
            name(&[ep(1, 2, "Pilot")], &cfg()),
            "Some Show - S01E02 - Pilot WEBDL-1080p"
        );
    }

    #[test]
    fn multi_episode_styles() {
        let eps = [ep(1, 2, "A (1)"), ep(1, 3, "B (2)"), ep(1, 4, "C")];
        let with = |style| {
            let mut c = cfg();
            c.multi_episode_style = style;
            c.standard_episode_format = "{Series Title} - S{season:00}E{episode:00}".into();
            name(&eps, &c)
        };
        assert_eq!(with(MultiEpisodeStyle::Extend), "Some Show - S01E02-03-04");
        assert_eq!(
            with(MultiEpisodeStyle::Duplicate),
            "Some Show - S01E02 - S01E03 - S01E04"
        );
        assert_eq!(with(MultiEpisodeStyle::Repeat), "Some Show - S01E02E03E04");
        assert_eq!(with(MultiEpisodeStyle::Scene), "Some Show - S01E02-E03-E04");
        assert_eq!(with(MultiEpisodeStyle::Range), "Some Show - S01E02-04");
        assert_eq!(
            with(MultiEpisodeStyle::PrefixedRange),
            "Some Show - S01E02-E04"
        );
    }

    #[test]
    fn multi_episode_titles_are_joined_and_part_markers_dropped() {
        let eps = [ep(1, 2, "Two Parter (1)"), ep(1, 3, "Two Parter (2)")];
        assert_eq!(
            name(&eps, &cfg()),
            "Some Show - S01E02-E03 - Two Parter WEBDL-1080p"
        );
    }

    #[test]
    fn daily_and_anime_formats() {
        let mut s = series();
        s.series_type = NamingSeriesType::Daily;
        let n = build_episode_file_name(&s, &[ep(2021, 63, "Guest")], &file(), 4, &cfg()).unwrap();
        assert_eq!(n, "Some Show - 2021-03-04 - Guest WEBDL-1080p");

        let mut a = series();
        a.series_type = NamingSeriesType::Anime;
        let mut c = cfg();
        c.anime_episode_format =
            "{Series Title} - S{season:00}E{episode:00} - {absolute:000} - {Episode Title}".into();
        let mut e = ep(1, 2, "Ep");
        e.absolute = Some(14);
        assert_eq!(
            build_episode_file_name(&a, &[e.clone()], &file(), 4, &c).unwrap(),
            "Some Show - S01E02 - 014 - Ep"
        );
        // Without an absolute number the standard format is used instead.
        e.absolute = None;
        assert_eq!(
            build_episode_file_name(&a, &[e], &file(), 4, &c).unwrap(),
            "Some Show - S01E02 - Ep WEBDL-1080p"
        );
    }

    #[test]
    fn path_patterns_make_folders_and_renaming_off_keeps_the_release_name() {
        let mut c = cfg();
        c.standard_episode_format =
            "{Series Title}/Season {season:00}/{Series Title} - {season}x{episode:00}".into();
        assert_eq!(
            name(&[ep(1, 2, "x")], &c),
            "Some Show/Season 01/Some Show - 1x02"
        );
        assert_eq!(
            name(&[ep(1, 2, "x")], &NamingConfig::default()),
            "Some.Show.S01E02.1080p.WEB-DL-GRP"
        );
    }

    #[test]
    fn long_episode_titles_are_cut_to_fit() {
        let long = "x".repeat(300);
        let n = name(&[ep(1, 2, &long)], &cfg());
        assert!(n.len() + 4 <= 255, "{}", n.len());
        assert!(n.contains("..."));
    }

    #[test]
    fn folders() {
        let c = NamingConfig::default();
        assert_eq!(build_series_folder(&series(), &c), "Some Show (2020)");
        assert_eq!(build_season_folder(&series(), 3, None, &c), "Season 3");
        assert_eq!(build_season_folder(&series(), 0, None, &c), "Specials");
    }
}
