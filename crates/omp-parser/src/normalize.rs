//! Title normalization for matching and slugs. Ported from Radarr `Parser`
//! (`CleanMovieTitle`, `NormalizeMovieTitle`, `NormalizeTitle`, `ToUrlSlug`)
//! and `StringExtensions.RemoveAccent` (GPL-3.0).

use std::sync::LazyLock;

use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

use crate::re::Re;

// Drops "a/an/the/and/or/of" in the middle of a title, plus every non-word char.
static NORMALIZE: LazyLock<Re> = LazyLock::new(|| {
    Re::ci(
        r"((?:\b|_)(?<!^|[^a-zA-Z0-9_']\w[^a-zA-Z0-9_'])([aà](?!$|[^a-zA-Z0-9_']\w[^a-zA-Z0-9_'])|an|the|and|or|of)(?!$)(?:\b|_))|\W|_",
    )
});
static SPECIAL_EPISODE_WORD: LazyLock<Re> =
    LazyLock::new(|| Re::ci(r"\b(part|special|edition|christmas)\b\s?"));
static PUNCTUATION: LazyLock<Re> = LazyLock::new(|| Re::new(r"[^\w\s]"));
static DUPLICATE_SPACES: LazyLock<Re> = LazyLock::new(|| Re::new(r"\s{2,}"));
static ARTICLE_WORD: LazyLock<Re> = LazyLock::new(|| Re::ci(r"^(a|an|the)\s"));
static SPECIAL_CHAR: LazyLock<Re> = LazyLock::new(|| Re::new(r"(\&|\:|\\|\/)+"));
static SLUG_INVALID: LazyLock<Re> = LazyLock::new(|| Re::new(r"[^a-z0-9\s\-_]"));

/// Removes diacritics: decompose, drop combining marks, recompose.
pub fn remove_accent(text: &str) -> String {
    text.nfd()
        .filter(|c| !is_combining_mark(*c))
        .nfc()
        .collect()
}

pub fn replace_german_umlauts(s: &str) -> String {
    s.replace('ä', "ae")
        .replace('ö', "oe")
        .replace('ü', "ue")
        .replace('Ä', "Ae")
        .replace('Ö', "Oe")
        .replace('Ü', "Ue")
        .replace('ß', "ss")
}

/// The key used to compare titles: lower case, no punctuation, no spaces, no
/// filler words in the middle, no accents. All-digit titles are kept as is.
pub fn clean_movie_title(title: &str) -> String {
    if title.trim().is_empty() || title.trim().parse::<i64>().is_ok() {
        return title.to_owned();
    }
    let stripped = NORMALIZE.replace_all(title, "").to_lowercase();
    remove_accent(&replace_german_umlauts(&stripped))
}

pub fn normalize_movie_title(title: &str) -> String {
    let title = SPECIAL_EPISODE_WORD.replace_all(title, "");
    let title = PUNCTUATION.replace_all(&title, " ");
    DUPLICATE_SPACES
        .replace_all(&title, " ")
        .trim()
        .to_lowercase()
}

pub fn normalize_title(title: &str) -> String {
    let title = PUNCTUATION.replace_all(title, "");
    let title = ARTICLE_WORD.replace_all(&title, "");
    let title = DUPLICATE_SPACES.replace_all(&title, " ");
    SPECIAL_CHAR.replace_all(&title, "").trim().to_lowercase()
}

/// URL slug with the defaults Radarr uses: trim and de-duplicate `-` and `_`.
pub fn to_url_slug(value: &str, invalid_dash_replacement: bool) -> String {
    to_url_slug_with(value, invalid_dash_replacement, "-_", "-_")
}

/// URL slug: lower case, accents removed, spaces to dashes, other invalid
/// characters dropped (or turned to dashes). `trim_end_chars` are trimmed from
/// both ends; any run of two or more characters from `dedupe_chars` collapses
/// to the run's last character. Empty lists disable the step.
pub fn to_url_slug_with(
    value: &str,
    invalid_dash_replacement: bool,
    trim_end_chars: &str,
    dedupe_chars: &str,
) -> String {
    let lowered = remove_accent(&value.to_lowercase());
    let dashed: String = lowered
        .chars()
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .collect();
    let replacement = if invalid_dash_replacement { "-" } else { "" };
    let cleaned = SLUG_INVALID.replace_all(&dashed, replacement);
    let trimmed = if trim_end_chars.is_empty() {
        cleaned.as_str()
    } else {
        let set: Vec<char> = trim_end_chars.chars().collect();
        cleaned.trim_matches(set.as_slice())
    };

    let mut out = String::with_capacity(trimmed.len());
    let mut run: Vec<char> = Vec::new();
    for c in trimmed.chars() {
        if dedupe_chars.contains(c) {
            run.push(c);
            continue;
        }
        flush_run(&mut out, &mut run);
        out.push(c);
    }
    flush_run(&mut out, &mut run);
    out
}

fn flush_run(out: &mut String, run: &mut Vec<char>) {
    match run.as_slice() {
        [] => {}
        [only] => out.push(*only),
        [.., last] => out.push(*last),
    }
    run.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_title_basics() {
        assert_eq!(clean_movie_title("Conan"), "conan");
        assert_eq!(clean_movie_title("Castle (2009)"), "castle2009");
        assert_eq!(clean_movie_title("Law_and_Order_SVU"), "lawordersvu");
        assert_eq!(clean_movie_title("90210"), "90210");
        assert_eq!(clean_movie_title("Carnivàle"), "carnivale");
        assert_eq!(clean_movie_title("Tokyo Ghoul A"), "tokyoghoula");
    }

    #[test]
    fn filler_words_only_in_the_middle() {
        for word in ["the", "and", "or", "an", "of", "a"] {
            for fmt in ["word.W.word", "word W word", "word-W-word"] {
                let t = fmt.replace('W', word);
                assert_eq!(clean_movie_title(&t), "wordword", "{t}");
            }
            for fmt in ["W.word.word", "W-word-word", "W word word"] {
                let t = fmt.replace('W', word);
                assert_eq!(clean_movie_title(&t), format!("{word}wordword"), "{t}");
            }
            for fmt in [
                "word.Wword",
                "word Wword",
                "word-Wword",
                "wordW.word",
                "wordW-word",
            ] {
                let t = fmt.replace('W', word);
                assert_eq!(clean_movie_title(&t), format!("word{word}word"), "{t}");
            }
        }
        for word in ["the", "and", "or", "an", "of"] {
            for fmt in ["word.word.W", "word-word-W", "word-word W"] {
                let t = fmt.replace('W', word);
                assert_eq!(clean_movie_title(&t), format!("wordword{word}"), "{t}");
            }
        }
    }

    #[test]
    fn a_inside_acronyms_is_kept() {
        for fmt in [
            "word.a.N.K.L.E.word",
            "word a N K L E word",
            "word-a-N-K-L-E-word",
        ] {
            assert_eq!(clean_movie_title(fmt), "wordankleword", "{fmt}");
        }
        for fmt in [
            "word.N.K.L.E.a.word",
            "word N K L E a word",
            "word-N-K-L-E-a-word",
        ] {
            assert_eq!(clean_movie_title(fmt), "wordnkleaword", "{fmt}");
        }
    }

    #[test]
    fn slugs() {
        assert_eq!(to_url_slug("tËst", false), "test");
        assert_eq!(to_url_slug("ßtest", false), "test");
        assert_eq!(
            to_url_slug("ThIS IS A MiXeD CaSe", false),
            "this-is-a-mixed-case"
        );
        assert_eq!(to_url_slug("test----", false), "test");
        assert_eq!(to_url_slug("test     result", false), "test-result");
        assert_eq!(to_url_slug("Test'Result", false), "testresult");
    }

    #[test]
    fn normalize_variants() {
        assert_eq!(
            normalize_movie_title("Movie: Part 2 - Special Edition"),
            "movie 2"
        );
        assert_eq!(normalize_title("The Movie: A & B"), "movie a b");
    }
}
