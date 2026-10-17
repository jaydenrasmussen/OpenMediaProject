//! Movie title parsing. Ported from Radarr `Parser.ParseMovieTitle` (GPL-3.0).
//!
//! The title patterns are tried in order and the first one that produces a
//! result wins, so their order is part of the behavior.

use std::sync::LazyLock;

use omp_core::{
    language::Language,
    quality::{Quality, QualityModel},
};

use crate::{
    common::{CLEAN_TORRENT_SUFFIX, WEBSITE_POSTFIX, WEBSITE_PREFIX},
    files::remove_file_extension,
    language::parse_languages,
    quality::{parse_quality, parse_quality_name},
    re::{Caps, Re},
    release_group::parse_release_group,
};

const EDITION: &str = r"\(?\b(?<edition>(((Recut.|Extended.|Ultimate.)?(Director.?s|Collector.?s|Theatrical|Ultimate|Extended|Despecialized|(Special|Rouge|Final|Assembly|Imperial|Diamond|Signature|Hunter|Rekall)(?=(.(Cut|Edition|Version)))|\d{2,3}(th)?.Anniversary)(?:.(Cut|Edition|Version))?(.(Extended|Uncensored|Remastered|Unrated|Uncut|Open.?Matte|IMAX|Fan.?Edit))?|((Uncensored|Remastered|Unrated|Uncut|Open?.Matte|IMAX|Fan.?Edit|Restored|((2|3|4)in1))))))\b\)?";

static REPORT_EDITION: LazyLock<Re> = LazyLock::new(|| Re::ci(&format!("^.+?{EDITION}")));

static HARDCODED_SUBS: LazyLock<Re> = LazyLock::new(|| {
    Re::ci(r"\b((?<hcsub>(\w+(?<!SOFT|MULTI|HORRIBLE)SUBS?))|(?<hc>(HC|SUBBED)))\b")
});

/// Title patterns, tried in order.
static TITLE_PATTERNS: LazyLock<Vec<Re>> = LazyLock::new(|| {
    vec![
        // Anime [Subgroup] with year explicitly in parentheses, e.g. [Sub] Title 1993 (1993) [VHS].
        // Must come before the generic anime+year pattern.
        Re::ci(
            r"^(?:\[(?<subgroup>.+?)\][-_. ]?)(?<title>(?![(\[]).+?)\s*\((?<year>(1(8|9)|20)\d{2})\).*?(?<hash>\[\w{8}\])?(?:$|\.)",
        ),
        // Anime [Subgroup] and Year
        Re::ci(
            r"^(?:\[(?<subgroup>.+?)\][-_. ]?)(?<title>(?![(\[]).+?)?(?:(?:[-_\W](?<![)\[!]))*(?<year>(1(8|9)|20)\d{2}(?!p|i|x|\d+|\]|\W\d+)))+.*?(?<hash>\[\w{8}\])?(?:$|\.)",
        ),
        // Anime [Subgroup] no year, versioned title, hash
        Re::ci(
            r"^(?:\[(?<subgroup>.+?)\][-_. ]?)(?<title>(?![(\[]).+?)((v)(?:\d{1,2})(?:([-_. ])))(\[.*)?(?:[\[(][^])])?.*?(?<hash>\[\w{8}\])(?:$|\.)",
        ),
        // Anime [Subgroup] no year, info in double sets of brackets, hash
        Re::ci(
            r"^(?:\[(?<subgroup>.+?)\][-_. ]?)(?<title>(?![(\[]).+?)(\[.*).*?(?<hash>\[\w{8}\])(?:$|\.)",
        ),
        // Anime [Subgroup] no year, info in parentheses or brackets, hash
        Re::ci(
            r"^(?:\[(?<subgroup>.+?)\][-_. ]?)(?<title>(?![(\[]).+)(?:[\[(][^])]).*?(?<hash>\[\w{8}\])(?:$|\.)",
        ),
        // German or TrueFrench tracker formats (missing year, ...). French was removed
        // upstream because it broke every movie with a French title.
        Re::ci(&format!(
            r"^(?<title>(?![(\[]).+?)((\W|_))({EDITION}.{{1,3}})?(?:(?<!(19|20)\d{{2}}.*?)(?<!(?:Good|The)[_ .-])(German|TrueFrench))(.+?)(?=((19|20)\d{{2}}|$))(?<year>(19|20)\d{{2}}(?!p|i|\d+|\]|\W\d+))?(\W+|_|$)(?!\\)"
        )),
        // Special, Despecialized, etc. edition movies, e.g. Mission.Impossible.3.Special.Edition.2011
        Re::ci(&format!(
            r"^(?<title>(?![(\[]).+?)?(?:(?:[-_\W](?<![)\[!]))*{EDITION}.{{1,3}}(?<year>(1(8|9)|20)\d{{2}}(?!p|i|\d+|\]|\W\d+)))+(\W+|_|$)(?!\\)"
        )),
        // Normal movie format, e.g. Mission.Impossible.3.2011
        Re::ci(
            r"^(?<title>(?![(\[]).+?)?(?:(?:[-_\W](?<![)\[!]))*(?<year>(1(8|9)|20)\d{2}(?!p|i|(1(8|9)|20)\d{2}|\]|\W(1(8|9)|20)\d{2})))+(\W+|_|$)(?!\\)",
        ),
        // PassThePopcorn torrent names: Star.Wars[PassThePopcorn]
        Re::ci(r"^(?<title>.+?)?(?:(?:[-_\W](?<![()\[!]))*(?<year>(\[\w *\])))+(\W+|_|$)(?!\\)"),
        // Some tool uses [] for years.
        Re::ci(
            r"^(?<title>(?![(\[]).+?)?(?:(?:[-_\W](?<![)!]))*(?<year>(1(8|9)|20)\d{2}(?!p|i|\d+|\W\d+)))+(\W+|_|$)(?!\\)",
        ),
        // Last resort for movies that have ( or [ in their title.
        Re::ci(
            r"^(?<title>.+?)?(?:(?:[-_\W](?<![)\[!]))*(?<year>(1(8|9)|20)\d{2}(?!p|i|\d+|\]|\W\d+)))+(\W+|_|$)(?!\\)",
        ),
    ]
});

/// Extra pattern for folder names: the year comes first.
static FOLDER_PATTERN: LazyLock<Re> = LazyLock::new(|| {
    Re::new(
        r"^(?:(?:[-_\W](?<![)!]))*(?<year>(19|20)\d{2}(?!p|i|\d+|\W\d+)))+(\W+|_|$)(?<title>.+?)?$",
    )
});

static REJECT_HASHED: LazyLock<Vec<Re>> = LazyLock::new(|| {
    vec![
        // md5 and mixed-case hashes
        Re::new(r"^[0-9a-zA-Z]{32}"),
        // shorter lower-case hashes
        Re::new(r"^[a-z0-9]{24}$"),
        // NZBGeek formats; strict because they are close to valid 101 numbering
        Re::new(r"^[A-Z]{11}\d{3}$"),
        Re::new(r"^[a-z]{12}\d{3}$"),
        Re::new(r"^Backup_\d{5,}S\d{2}-\d{2}$"),
        Re::new(r"^123$"),
        Re::ci(r"^abc$"),
        Re::ci(r"^abc[-_. ]xyz"),
        Re::ci(r"^b00bs$"),
    ]
});

static REVERSED_TITLE: LazyLock<Re> = LazyLock::new(|| Re::new(r"(?:^|[-._ ])(p027|p0801)[-._ ]"));
static ALTERNATIVE_TITLE: LazyLock<Re> = LazyLock::new(|| Re::ci(r"[ ]+(?:AKA|\/)[ ]+"));
static BRACKETED_ALTERNATIVE_TITLE: LazyLock<Re> =
    LazyLock::new(|| Re::ci(r"(?<a>.*) \([ ]*AKA[ ]+(?<b>.*)\)"));
static NORMALIZE_ALTERNATIVE_TITLE: LazyLock<Re> =
    LazyLock::new(|| Re::ci(r"[ ]+(?:A\.K\.A\.)[ ]+"));
static REPORT_IMDB_ID: LazyLock<Re> = LazyLock::new(|| Re::ci(r"(?<imdbid>tt\d{7,8})"));
static REPORT_TMDB_ID: LazyLock<Re> = LazyLock::new(|| Re::ci(r"tmdb(id)?-(?<tmdbid>\d+)"));
static SIMPLE_TITLE: LazyLock<Re> = LazyLock::new(|| {
    Re::ci(
        r"(?:(480|540|576|720|1080|2160)[ip]|[xh][\W_]?26[45]|DD\W?5\W1|[<>?*]|848x480|1280x720|1920x1080|3840x2160|4096x2160|(8|10)b(it)?|10-bit)\s*?(?![a-b0-9])",
    )
});
static SIMPLE_RELEASE_TITLE: LazyLock<Re> = LazyLock::new(|| Re::ci(r"\s*(?:[<>?*|])"));
static CLEAN_QUALITY_BRACKETS: LazyLock<Re> = LazyLock::new(|| Re::ci(r"\[[a-z0-9 ._-]+\]$"));
static REQUEST_INFO: LazyLock<Re> = LazyLock::new(|| Re::new(r"^(?:\[.+?\])+"));

#[derive(Debug, Clone, Default)]
pub struct ParsedMovieInfo {
    /// First entry is the primary title; the rest are `AKA` alternatives.
    pub movie_titles: Vec<String>,
    pub original_title: String,
    pub release_title: String,
    pub simple_release_title: String,
    pub quality: QualityModel,
    pub languages: Vec<Language>,
    pub release_group: Option<String>,
    pub release_hash: String,
    pub edition: String,
    pub year: i32,
    pub imdb_id: String,
    pub tmdb_id: i32,
    pub hardcoded_subs: Option<String>,
}

impl ParsedMovieInfo {
    pub fn primary_title(&self) -> Option<&str> {
        self.movie_titles.first().map(String::as_str)
    }
}

impl std::fmt::Display for ParsedMovieInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} - {} {}",
            self.primary_title().unwrap_or_default(),
            self.year,
            self.quality.quality
        )
    }
}

fn validate_before_parsing(title: &str) -> bool {
    let lower = title.to_lowercase();
    if lower.contains("password") && lower.contains("yenc") {
        return false;
    }
    if !title.chars().any(char::is_alphanumeric) {
        return false;
    }
    let without_extension = remove_file_extension(title);
    !REJECT_HASHED.iter().any(|r| r.is_match(&without_extension))
}

/// Parses a movie path: file name first, then directory + file name, then
/// the directory name alone.
pub fn parse_movie_path(path: &str) -> Option<ParsedMovieInfo> {
    let normalized = path.replace('\\', "/");
    let (dir, file) = match normalized.rsplit_once('/') {
        Some((d, f)) => (d, f),
        None => ("", normalized.as_str()),
    };
    let dir_name = dir.rsplit('/').next().unwrap_or_default();
    let extension = crate::files::path_extension(file);

    parse_movie_title(file, true)
        .or_else(|| parse_movie_title(&format!("{dir_name} {file}"), false))
        .or_else(|| parse_movie_title(&format!("{dir_name}{extension}"), false))
}

/// `is_dir` also tries the "year first" folder pattern.
pub fn parse_movie_title(title: &str, is_dir: bool) -> Option<ParsedMovieInfo> {
    let original_title = title;
    if !validate_before_parsing(title) {
        return None;
    }

    let mut title = title.to_owned();
    if REVERSED_TITLE.is_match(&title) {
        // Upstream's reversal is broken (it formats a char array); we do what
        // it meant to do: reverse the name and keep the extension.
        let without_extension = remove_file_extension(&title);
        let reversed: String = without_extension.chars().rev().collect();
        title = format!("{reversed}{}", &title[without_extension.len()..]);
    }

    let release_title = remove_file_extension(&title);
    let release_title = release_title
        .trim_matches(['-', '_'])
        .replace('【', "[")
        .replace('】', "]");

    let mut simple_title = SIMPLE_TITLE.replace_all(&release_title, "");
    simple_title = WEBSITE_PREFIX.replace_all(&simple_title, "");
    simple_title = WEBSITE_POSTFIX.replace_all(&simple_title, "");
    simple_title = CLEAN_TORRENT_SUFFIX.replace_all(&simple_title, "");
    simple_title = CLEAN_QUALITY_BRACKETS.replace_with(&simple_title, |c| {
        if parse_quality_name(c.whole_str()).quality != Quality::UNKNOWN {
            String::new()
        } else {
            c.whole_str().to_owned()
        }
    });

    let folder = is_dir.then(|| &*FOLDER_PATTERN);
    for pattern in TITLE_PATTERNS.iter().chain(folder) {
        let Some(m) = pattern.captures(&simple_title) else {
            continue;
        };
        let Some(mut result) = parse_match(&m) else {
            continue;
        };

        // Replace the title in the release title with a placeholder so the
        // release group and language parsers cannot mistake title words for tags.
        let mut simple_release_title = SIMPLE_RELEASE_TITLE.replace_all(&release_title, "");
        let title_replacement = m
            .name("title")
            .map(str::to_owned)
            .or_else(|| result.primary_title().map(str::to_owned))
            .unwrap_or_default();
        if !title_replacement.trim().is_empty() {
            let placeholder = if title_replacement.contains('.') {
                "A.Movie"
            } else {
                "A Movie"
            };
            match m.span("title") {
                Some(span) => {
                    // .NET indexes strings in UTF-16 units, and the span is taken
                    // from `simple_title` but applied to `simple_release_title`.
                    // Keep that, including the failure when it does not fit.
                    let start = simple_title[..span.start].encode_utf16().count();
                    let len = simple_title[span.clone()].encode_utf16().count();
                    let mut units: Vec<u16> = simple_release_title.encode_utf16().collect();
                    if start + len > units.len() {
                        return None;
                    }
                    units.splice(start..start + len, placeholder.encode_utf16());
                    simple_release_title = String::from_utf16_lossy(&units);
                }
                None => {
                    simple_release_title =
                        simple_release_title.replace(&title_replacement, placeholder);
                }
            }
        }

        result.release_group = parse_release_group(&simple_release_title);
        if let Some(sub) = m.name("subgroup").filter(|s| !s.trim().is_empty()) {
            result.release_group = Some(sub.to_owned());
        }

        result.hardcoded_subs = parse_hardcode_subs(&title);
        // The release group is masked so a group named e.g. "GERMAN" is not read as a language.
        result.languages = parse_languages(&match result.release_group.as_deref() {
            Some(group) if !group.trim().is_empty() => {
                simple_release_title.replace(group, "RlsGrp")
            }
            _ => simple_release_title.clone(),
        });
        result.quality = parse_quality(&title);

        if result.edition.trim().is_empty() {
            result.edition = parse_edition(&simple_release_title);
        }

        result.release_hash = release_hash(&m);
        result.original_title = original_title.to_owned();
        result.release_title = release_title.clone();
        result.imdb_id = parse_imdb_id(&simple_release_title);
        result.tmdb_id = parse_tmdb_id(&simple_release_title);
        result.simple_release_title = simple_release_title;
        return Some(result);
    }
    None
}

fn parse_match(m: &Caps<'_>) -> Option<ParsedMovieInfo> {
    let raw_title = m.name("title")?;
    if raw_title == "(" {
        return None;
    }

    let name = raw_title.replace('_', " ");
    let name = NORMALIZE_ALTERNATIVE_TITLE.replace_all(&name, " AKA ");
    let name = REQUEST_INFO.replace_all(&name, "");
    let name = name.trim_matches(' ');

    let parts: Vec<&str> = name.split('.').collect();
    let mut movie_name = String::new();
    let mut previous_acronym = false;
    for (n, part) in parts.iter().enumerate() {
        let next_part = parts.get(n + 1).copied().unwrap_or("");
        let is_number = |s: &str| s.trim().parse::<i32>().is_ok();
        let lower = part.to_lowercase();

        // Single letters ("U.N.C.L.E."), a lone "a" next to one, and "Dr." keep their dot.
        let single_letter = part.chars().count() == 1
            && lower != "a"
            && !is_number(part)
            && (previous_acronym || n < parts.len() - 1)
            && (previous_acronym || next_part.chars().count() != 1 || !is_number(next_part));
        let article_in_acronym =
            lower == "a" && (previous_acronym || next_part.chars().count() == 1);

        if single_letter || article_in_acronym || lower == "dr" {
            movie_name.push_str(part);
            movie_name.push('.');
            previous_acronym = true;
        } else {
            if previous_acronym {
                movie_name.push(' ');
                previous_acronym = false;
            }
            movie_name.push_str(part);
            movie_name.push(' ');
        }
    }
    let movie_name = movie_name.trim_matches(' ').to_owned();

    let year = m
        .name("year")
        .and_then(|y| y.trim().parse::<i32>().ok())
        .unwrap_or(0);

    let mut result = ParsedMovieInfo {
        year,
        ..Default::default()
    };
    if let Some(edition) = m.name("edition") {
        result.edition = edition.replace('.', " ");
    }

    let unbracketed = BRACKETED_ALTERNATIVE_TITLE.replace_with(&movie_name, |c| {
        format!(
            "{} AKA {}",
            c.name("a").unwrap_or_default(),
            c.name("b").unwrap_or_default()
        )
    });
    let mut titles = vec![movie_name.clone()];
    titles.extend(
        ALTERNATIVE_TITLE
            .split(&unbracketed)
            .into_iter()
            .filter(|alt| !alt.trim().is_empty() && *alt != movie_name)
            .map(str::to_owned),
    );
    result.movie_titles = titles;
    Some(result)
}

pub fn parse_imdb_id(title: &str) -> String {
    match REPORT_IMDB_ID
        .captures(title)
        .and_then(|c| c.name("imdbid"))
    {
        Some(id) if id.len() == 9 || id.len() == 10 => id.to_owned(),
        _ => String::new(),
    }
}

pub fn parse_tmdb_id(title: &str) -> i32 {
    REPORT_TMDB_ID
        .captures(title)
        .and_then(|c| c.name("tmdbid")?.parse().ok())
        .unwrap_or(0)
}

pub fn parse_edition(title: &str) -> String {
    REPORT_EDITION
        .captures(title)
        .and_then(|c| c.name("edition").map(|e| e.replace('.', " ")))
        .filter(|e| !e.trim().is_empty())
        .unwrap_or_default()
}

pub fn parse_hardcode_subs(title: &str) -> Option<String> {
    let last = HARDCODED_SUBS.captures_all(title).pop()?;
    if let Some(sub) = last.name("hcsub") {
        Some(sub.to_owned())
    } else if last.has("hc") {
        Some("Generic Hardcoded Subs".to_owned())
    } else {
        None
    }
}

fn release_hash(m: &Caps<'_>) -> String {
    match m.name("hash") {
        Some(h) => {
            let value = h.trim_matches(['[', ']']);
            if value == "1280x720" {
                String::new()
            } else {
                value.to_owned()
            }
        }
        None => String::new(),
    }
}

/// Normalizes an IMDb id to `tt` + at least 7 digits, or `None`.
pub fn normalize_imdb_id(imdb_id: &str) -> Option<String> {
    static IMDB: LazyLock<Re> = LazyLock::new(|| Re::new(r"^(\d{1,10}|(tt)\d{1,10})$"));
    if !IMDB.is_match(imdb_id) || imdb_id.len() <= 2 {
        return None;
    }
    let digits = imdb_id.replace("tt", "");
    Some(format!("tt{digits:0>7}"))
}
