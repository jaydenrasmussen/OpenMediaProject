use std::sync::LazyLock;

use omp_parser::re::Re;
use serde::{Deserialize, Serialize};

use crate::{
    iso,
    text::{
        clean_file_name, clean_folder_name, clean_title, clean_title_the, collapse_separators,
        title_first_character, title_the,
    },
};

/// Used for `{Release Group}` when the file has none and the pattern has no
/// prefix or suffix around the token.
pub const DEFAULT_RELEASE_GROUP: &str = "OpenMediaProject";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColonReplacement {
    Delete,
    Dash,
    SpaceDash,
    SpaceDashSpace,
    /// ": " becomes " - ", a bare ":" becomes "-".
    Smart,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NamingConfig {
    pub rename_movies: bool,
    pub replace_illegal_characters: bool,
    pub colon_replacement: ColonReplacement,
    pub standard_movie_format: String,
    pub movie_folder_format: String,
}

impl Default for NamingConfig {
    fn default() -> Self {
        Self {
            rename_movies: false,
            replace_illegal_characters: true,
            colon_replacement: ColonReplacement::Smart,
            movie_folder_format: "{Movie Title} ({Release Year})".into(),
            standard_movie_format: "{Movie Title} ({Release Year}) {Quality Full}".into(),
        }
    }
}

/// What the movie tokens need to know.
#[derive(Debug, Clone, Default)]
pub struct NamingMovie {
    pub title: String,
    pub original_title: Option<String>,
    pub year: i32,
    pub imdb_id: Option<String>,
    pub tmdb_id: i64,
    pub certification: Option<String>,
    pub collection_title: Option<String>,
    /// (ISO 639-1 code, title) pairs for `{Movie Title:FR}`.
    pub translations: Vec<(String, String)>,
}

/// Already-formatted media details for the `{MediaInfo ...}` tokens.
#[derive(Debug, Clone, Default)]
pub struct NamingMedia {
    pub video_codec: String,
    pub video_bit_depth: i32,
    pub audio_codec: String,
    pub audio_channels: f64,
    pub audio_languages: Vec<String>,
    pub subtitles: Vec<String>,
    pub is_3d: bool,
    pub dynamic_range: String,
    pub dynamic_range_type: String,
}

/// What the file tokens need to know.
#[derive(Debug, Clone, Default)]
pub struct NamingFile {
    /// Display name of the quality ("Bluray-1080p"); `None` if unknown.
    pub quality_title: Option<String>,
    pub revision_version: i32,
    pub revision_real: i32,
    pub release_group: Option<String>,
    pub scene_name: Option<String>,
    pub relative_path: Option<String>,
    pub path: Option<String>,
    pub edition: Option<String>,
    pub media: Option<NamingMedia>,
    /// Names of the custom formats that opted in to being part of file names.
    pub custom_formats: Vec<String>,
}

static TOKEN: LazyLock<Re> = LazyLock::new(|| {
    Re::ci(
        r"(?<tag>\{[-{ ._\[(]*(?:imdb(?:id)?-|edition-))?\{(?<prefix>[-{ ._\[(]*)(?<token>(?:[a-z0-9]+)(?:(?<separator>[- ._]+)(?:[a-z0-9]+))?)(?::(?<customFormat>[ ,a-z0-9|+-]+(?<![- ])))?(?<suffix>[-} ._)\]]*)\}",
    )
});
static TRIM_TRAILING: LazyLock<Re> = LazyLock::new(|| Re::new(r"[- ._]+$"));
static RESERVED: LazyLock<Re> =
    LazyLock::new(|| Re::ci(r"^(?:aux|com[1-9]|con|lpt[1-9]|nul|prn)\."));
static ORDINAL: LazyLock<Re> =
    LazyLock::new(|| Re::ci(r"((?:\b|_)\d{1,3}(?:st|th|rd|nd|mm)(?:\b|_))"));
static UPPER_WORDS: LazyLock<Re> =
    LazyLock::new(|| Re::ci(r"((?:\b|_)(?:IMAX|3D|SDR|HDR|DV)(?:\b|_))"));

struct TokenMatch<'a> {
    prefix: &'a str,
    suffix: &'a str,
    /// The `:...` part, if any.
    custom: Option<&'a str>,
}

impl TokenMatch<'_> {
    /// A fallback that only applies when the pattern puts nothing around the token.
    fn default_value(&self, value: &str) -> String {
        if self.prefix.is_empty() && self.suffix.is_empty() {
            value.to_owned()
        } else {
            String::new()
        }
    }
}

struct Context<'a> {
    movie: &'a NamingMovie,
    file: Option<&'a NamingFile>,
    multiple_tokens: bool,
}

/// Token names compare without spaces, punctuation or case.
fn simplify(token: &str) -> String {
    token
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn max_length(formatter: Option<&str>) -> i32 {
    formatter.and_then(|f| f.trim().parse().ok()).unwrap_or(0)
}

/// Shortens `input` for a `:N` (keep the start) or `:-N` (keep the end)
/// formatter, marking the cut with `{ellipsis}`.
fn truncate(input: &str, formatter: Option<&str>) -> String {
    if input.trim().is_empty() {
        return String::new();
    }
    let max = max_length(formatter);
    let chars: Vec<char> = input.chars().collect();
    let limit = max.unsigned_abs() as usize;
    if max == 0 || chars.len() <= limit {
        return input.to_owned();
    }
    let keep = limit.saturating_sub(3);
    if max < 0 {
        let tail: String = chars[chars.len() - keep.min(chars.len())..]
            .iter()
            .collect();
        format!("{{ellipsis}}{}", tail.trim_start_matches([' ', '.']))
    } else {
        let head: String = chars[..keep].iter().collect();
        format!("{}{{ellipsis}}", head.trim_end_matches([' ', '.']))
    }
}

fn language_title(movie: &NamingMovie, codes: Option<&str>) -> String {
    if let Some(codes) = codes.filter(|c| !c.trim().is_empty()) {
        for code in codes.split('|') {
            let code = code.to_lowercase();
            if !iso::is_known_two_letter(&code) {
                continue;
            }
            return movie
                .translations
                .iter()
                .find(|(l, _)| l.eq_ignore_ascii_case(&code))
                .map_or_else(|| movie.title.clone(), |(_, t)| t.clone());
        }
    }
    movie.title.clone()
}

/// The `{MediaInfo ...Languages}` value: two-letter upper case codes joined
/// with `+`, optionally filtered (`EN+DE`, `-EN`, `EN+` for "and others").
fn languages_token(
    languages: &[String],
    filter: Option<&str>,
    skip_english_only: bool,
    quoted: bool,
) -> String {
    let mut tokens: Vec<String> = Vec::new();
    for item in languages {
        let item = item.trim();
        if item.is_empty() || item == "und" {
            continue;
        }
        let mapped = iso::two_letter(item).map_or_else(|| item.to_owned(), str::to_uppercase);
        if !tokens.contains(&mapped) {
            tokens.push(mapped);
        }
    }
    let filter = filter.filter(|f| !f.trim().is_empty());
    let mut filtered: Vec<String> = match filter {
        Some(f) if f.starts_with('-') => {
            let excluded: Vec<&str> = f.split('-').collect();
            tokens
                .iter()
                .filter(|t| !excluded.contains(&t.as_str()))
                .cloned()
                .collect()
        }
        Some(f) => {
            let mut out: Vec<String> = Vec::new();
            for wanted in f.split('+') {
                if tokens.iter().any(|t| t == wanted) && !out.iter().any(|o| o == wanted) {
                    out.push(wanted.to_owned());
                }
            }
            out
        }
        None => tokens.clone(),
    };
    if filter.is_some_and(|f| f.ends_with('+')) && filtered.len() != tokens.len() {
        filtered.push("--".into());
    }
    if skip_english_only && filtered.len() == 1 && filtered[0] == "EN" {
        return String::new();
    }
    let joined = filtered.join("+");
    if quoted && !joined.is_empty() {
        format!("[{joined}]")
    } else {
        joined
    }
}

fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if c.is_alphabetic() && start {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        start = !c.is_alphanumeric();
    }
    out
}

fn edition_token(edition: &str) -> String {
    let titled = title_case(&edition.to_lowercase());
    let titled = ORDINAL.replace_with(&titled, |c| c.whole_str().to_lowercase());
    UPPER_WORDS.replace_with(&titled, |c| c.whole_str().to_uppercase())
}

fn custom_formats_token(names: &[String], filter: Option<&str>) -> String {
    let selected: Vec<&String> = match filter.filter(|f| !f.trim().is_empty()) {
        Some(f) if f.starts_with('-') => {
            let excluded: Vec<&str> = f[1..].split(',').collect();
            names
                .iter()
                .filter(|n| !excluded.contains(&n.as_str()))
                .collect()
        }
        Some(f) => {
            let wanted: Vec<&str> = f.split(',').collect();
            names
                .iter()
                .filter(|n| wanted.contains(&n.as_str()))
                .collect()
        }
        None => names.iter().collect(),
    };
    selected
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn file_stem(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_owned(),
        _ => name.to_owned(),
    }
}

fn original_file_name(file: &NamingFile, multiple_tokens: bool) -> String {
    if multiple_tokens {
        return String::new();
    }
    match file
        .relative_path
        .as_deref()
        .filter(|p| !p.trim().is_empty())
    {
        Some(rel) => file_stem(rel),
        None => file.path.as_deref().map(file_stem).unwrap_or_default(),
    }
}

fn original_title(file: &NamingFile, multiple_tokens: bool) -> String {
    let defaults = NamingConfig::default();
    match file.scene_name.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(scene) => clean_file_name(scene, &defaults),
        None => clean_file_name(&original_file_name(file, multiple_tokens), &defaults),
    }
}

impl Context<'_> {
    fn value(&self, token: &str, m: &TokenMatch<'_>) -> String {
        let movie = self.movie;
        let custom = m.custom;
        match token {
            "movietitle" => truncate(&language_title(movie, custom), custom),
            "moviecleantitle" => truncate(&clean_title(&language_title(movie, custom)), custom),
            "movietitlethe" => truncate(&title_the(&movie.title), custom),
            "moviecleantitlethe" => truncate(&clean_title_the(&movie.title), custom),
            "movietitlefirstcharacter" => {
                title_first_character(&title_the(&language_title(movie, custom)))
            }
            "movieoriginaltitle" => {
                truncate(movie.original_title.as_deref().unwrap_or_default(), custom)
            }
            "moviecleanoriginaltitle" => truncate(
                &clean_title(movie.original_title.as_deref().unwrap_or_default()),
                custom,
            ),
            "moviecertification" => movie.certification.clone().unwrap_or_default(),
            "moviecollection" => truncate(
                movie.collection_title.as_deref().unwrap_or_default(),
                custom,
            ),
            "moviecollectionthe" => truncate(
                &title_the(movie.collection_title.as_deref().unwrap_or_default()),
                custom,
            ),
            "moviecleancollectionthe" => truncate(
                &clean_title_the(movie.collection_title.as_deref().unwrap_or_default()),
                custom,
            ),
            "releaseyear" if movie.year != 0 => movie.year.to_string(),
            "imdbid" => movie.imdb_id.clone().unwrap_or_default(),
            "tmdbid" => movie.tmdb_id.to_string(),
            _ => self
                .file
                .map(|f| self.file_value(f, token, m))
                .unwrap_or_default(),
        }
    }

    fn file_value(&self, file: &NamingFile, token: &str, m: &TokenMatch<'_>) -> String {
        let custom = m.custom;
        let proper = if file.revision_version > 1 {
            "Proper"
        } else {
            ""
        };
        let real = if file.revision_real > 0 { "REAL" } else { "" };
        match token {
            "originaltitle" => original_title(file, self.multiple_tokens),
            "originalfilename" => original_file_name(file, self.multiple_tokens),
            "releasegroup" => match file
                .release_group
                .as_deref()
                .filter(|g| !g.trim().is_empty())
            {
                Some(g) => truncate(g, custom),
                None => m.default_value(DEFAULT_RELEASE_GROUP),
            },
            "qualityfull" => file
                .quality_title
                .as_ref()
                .map(|t| format!("{t} {proper} {real}"))
                .unwrap_or_default(),
            "qualitytitle" => file.quality_title.clone().unwrap_or_default(),
            "qualityproper" if file.quality_title.is_some() => proper.to_owned(),
            "qualityreal" if file.quality_title.is_some() => real.to_owned(),
            "editiontags" => match file.edition.as_deref().filter(|e| !e.trim().is_empty()) {
                Some(e) => truncate(&edition_token(e), custom),
                None => String::new(),
            },
            "customformats" => custom_formats_token(&file.custom_formats, custom),
            "customformat" => custom
                .and_then(|c| {
                    file.custom_formats
                        .iter()
                        .find(|n| n.as_str() == c)
                        .cloned()
                })
                .unwrap_or_default(),
            _ => file
                .media
                .as_ref()
                .map(|mi| media_value(mi, token, custom))
                .unwrap_or_default(),
        }
    }
}

fn media_value(mi: &NamingMedia, token: &str, custom: Option<&str>) -> String {
    let audio = || languages_token(&mi.audio_languages, custom, true, true);
    let subs = || languages_token(&mi.subtitles, custom, false, true);
    match token {
        "mediainfovideo" | "mediainfovideocodec" => mi.video_codec.clone(),
        "mediainfovideobitdepth" => if mi.video_bit_depth > 0 {
            mi.video_bit_depth
        } else {
            8
        }
        .to_string(),
        "mediainfoaudio" | "mediainfoaudiocodec" => mi.audio_codec.clone(),
        "mediainfoaudiochannels" if mi.audio_channels > 0.0 => format!("{:.1}", mi.audio_channels),
        "mediainfoaudiolanguages" => audio(),
        "mediainfoaudiolanguagesall" => languages_token(&mi.audio_languages, custom, false, true),
        "mediainfosubtitlelanguages" | "mediainfosubtitlelanguagesall" => subs(),
        "mediainfo3d" if mi.is_3d => "3D".into(),
        "mediainfosimple" => format!("{} {}", mi.video_codec, mi.audio_codec),
        "mediainfofull" => format!(
            "{} {}{} {}",
            mi.video_codec,
            mi.audio_codec,
            audio(),
            subs()
        ),
        "mediainfovideodynamicrange" => mi.dynamic_range.clone(),
        "mediainfovideodynamicrangetype" => mi.dynamic_range_type.clone(),
        _ => String::new(),
    }
}

fn replace_tokens(pattern: &str, ctx: &Context<'_>, config: &NamingConfig) -> String {
    TOKEN.replace_with(pattern, |c| {
        let token = c.name("token").unwrap_or_default();
        let separator = c.name("separator").unwrap_or_default();
        let m = TokenMatch {
            prefix: c.name("prefix").unwrap_or_default(),
            suffix: c.name("suffix").unwrap_or_default(),
            custom: c.name("customFormat").filter(|s| !s.trim().is_empty()),
        };
        let mut text = ctx.value(&simplify(token), &m).trim().to_owned();

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
        let text = clean_file_name(&text, config);
        if text.trim().is_empty() {
            String::new()
        } else {
            format!(
                "{}{}{}{}",
                c.name("tag").unwrap_or_default(),
                m.prefix,
                text,
                m.suffix
            )
        }
    })
}

fn finish_ellipsis(s: &str) -> String {
    // Radarr only matches the lower case form; a pattern like {MOVIE TITLE:10}
    // upper-cases the marker, so match any case.
    static ELLIPSIS: LazyLock<Re> = LazyLock::new(|| Re::ci(r"\{ellipsis\}"));
    ELLIPSIS.replace_all(s, "...")
}

fn replace_reserved_device_names(s: &str) -> String {
    RESERVED.replace_with(s, |c| c.whole_str().replace('.', "_"))
}

fn split_pattern(pattern: &str) -> impl Iterator<Item = &str> {
    pattern.split(['\\', '/']).filter(|s| !s.is_empty())
}

/// The file name (no extension) for a movie file. With renaming off this is
/// the original release name. A path pattern (`Folder/File`) yields a
/// relative path with `/` separators.
pub fn build_file_name(
    movie: &NamingMovie,
    file: &NamingFile,
    config: &NamingConfig,
) -> Result<String, String> {
    if !config.rename_movies {
        return Ok(original_title(file, false));
    }
    if config.standard_movie_format.trim().is_empty() {
        return Err("Standard movie format cannot be empty".into());
    }
    let pattern = &config.standard_movie_format;
    let ctx = Context {
        movie,
        file: Some(file),
        multiple_tokens: TOKEN.captures_all(pattern).len() > 1,
    };
    let mut components = Vec::new();
    for part in split_pattern(pattern) {
        let component = replace_tokens(part, &ctx, config);
        let component = collapse_separators(component.trim());
        let component = TRIM_TRAILING.replace_all(&component, "");
        let component = replace_reserved_device_names(&finish_ellipsis(&component));
        if !component.trim().is_empty() {
            components.push(component);
        }
    }
    Ok(components.join("/"))
}

/// The movie's folder (possibly several levels) from the folder pattern.
pub fn build_movie_folder(movie: &NamingMovie, config: &NamingConfig) -> String {
    let ctx = Context {
        movie,
        file: None,
        multiple_tokens: false,
    };
    let mut components = Vec::new();
    for part in split_pattern(&config.movie_folder_format) {
        let component = clean_folder_name(&replace_tokens(part, &ctx, config));
        let component = replace_reserved_device_names(&finish_ellipsis(&component));
        if !component.trim().is_empty() {
            components.push(component);
        }
    }
    components.join("/")
}
