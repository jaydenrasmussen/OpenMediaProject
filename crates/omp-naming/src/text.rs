//! String helpers behind the naming tokens.

use std::sync::LazyLock;

use omp_parser::{normalize::remove_accent, re::Re};

use crate::builder::{ColonReplacement, NamingConfig};

static CLEANUP: LazyLock<Re> = LazyLock::new(|| Re::new(r"([- ._])(\1)+"));
static SCENIFY_REMOVE: LazyLock<Re> = LazyLock::new(|| {
    Re::ci(
        r#"(?<=\s)(,|<|>|\/|\\|;|:|'|"|\||`|’|~|!|\?|@|$|%|^|\*|-|_|=){1}(?=\s)|('|`|’|:|\?|,)(?=(?:(?:s|m|t|ve|ll|d|re)\s)|\s|$)|(\(|\)|\[|\]|\{|\})"#,
    )
});
static TITLE_PREFIX: LazyLock<Re> =
    LazyLock::new(|| Re::ci(r"^(?<article>The|An|A) (?<rest>.*?)(?<tail>(?: *\([^)]+\))*)$"));

const BAD: [&str; 8] = ["\\", "/", "<", ">", "?", "*", "|", "\""];
const GOOD: [&str; 8] = ["+", "+", "", "", "!", "-", "", ""];

/// Collapses runs of the same separator (`..`, `--`, `  `) to one.
pub(crate) fn collapse_separators(s: &str) -> String {
    CLEANUP.replace_with(s, |c| c.whole_str().chars().next().unwrap().to_string())
}

/// Strips accents, and also folds letters that have no decomposition (the
/// package Radarr uses does the same: `Æ` becomes `AE`).
fn remove_diacritics(text: &str) -> String {
    let folded: String = text
        .chars()
        .map(|c| match c {
            'Æ' => "AE".to_owned(),
            'æ' => "ae".to_owned(),
            'Œ' => "OE".to_owned(),
            'œ' => "oe".to_owned(),
            'Ø' => "O".to_owned(),
            'ø' => "o".to_owned(),
            'Đ' | 'Ð' => "D".to_owned(),
            'đ' | 'ð' => "d".to_owned(),
            'Ł' => "L".to_owned(),
            'ł' => "l".to_owned(),
            'ß' => "ss".to_owned(),
            'Þ' => "TH".to_owned(),
            'þ' => "th".to_owned(),
            'ı' => "i".to_owned(),
            c => c.to_string(),
        })
        .collect();
    remove_accent(&folded)
}

/// Removes characters that make titles awkward in file names.
pub fn clean_title(title: &str) -> String {
    let t = title.replace('&', "and").replace('/', " ");
    remove_diacritics(&SCENIFY_REMOVE.replace_all(&t, ""))
}

/// "The Mist (2007)" -> "Mist, The (2007)".
pub fn title_the(title: &str) -> String {
    if title.trim().is_empty() {
        return String::new();
    }
    TITLE_PREFIX.replace_with(title, |c| {
        format!(
            "{}, {}{}",
            c.name("rest").unwrap_or_default(),
            c.name("article").unwrap_or_default(),
            c.name("tail").unwrap_or_default()
        )
    })
}

pub fn clean_title_the(title: &str) -> String {
    if title.trim().is_empty() {
        return String::new();
    }
    match TITLE_PREFIX.captures(title) {
        Some(c) => format!(
            "{}, {}{}",
            clean_title(c.name("rest").unwrap_or_default()).trim(),
            c.name("article").unwrap_or_default(),
            clean_title(c.name("tail").unwrap_or_default())
        ),
        None => clean_title(title),
    }
}

/// First letter or digit of a title, upper case and without accents; tries
/// the second character if the first is punctuation, else `_`.
pub(crate) fn title_first_character(title: &str) -> String {
    for c in title.chars().take(2) {
        if c.is_alphanumeric() {
            let up: String = c.to_uppercase().collect();
            return remove_diacritics(&up)
                .chars()
                .next()
                .map(String::from)
                .unwrap_or_default();
        }
    }
    "_".into()
}

/// Makes a name safe for a file system, following the colon and illegal
/// character settings.
pub fn clean_file_name(name: &str, config: &NamingConfig) -> String {
    let mut result = name.to_owned();
    if config.replace_illegal_characters {
        if config.colon_replacement == ColonReplacement::Smart {
            result = result.replace(": ", " - ").replace(':', "-");
        } else {
            let replacement = match config.colon_replacement {
                ColonReplacement::Dash => "-",
                ColonReplacement::SpaceDash => " -",
                ColonReplacement::SpaceDashSpace => " - ",
                _ => "",
            };
            result = result.replace(':', replacement);
        }
    } else {
        result = result.replace(':', "");
    }
    for (bad, good) in BAD.iter().zip(GOOD) {
        result = result.replace(
            bad,
            if config.replace_illegal_characters {
                good
            } else {
                ""
            },
        );
    }
    result
        .trim_start_matches([' ', '.'])
        .trim_end_matches(' ')
        .to_owned()
}

/// Folder names keep the pattern's own punctuation but drop repeated
/// separators and surrounding spaces and dots.
pub fn clean_folder_name(name: &str) -> String {
    collapse_separators(name)
        .trim_matches([' ', '.'])
        .to_owned()
}
