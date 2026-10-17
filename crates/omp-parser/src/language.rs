//! Language detection from release names. Ported from Radarr
//! `LanguageParser.ParseLanguages` (GPL-3.0). Subtitle file parsing is not
//! ported yet.

use std::sync::LazyLock;

use omp_core::language::Language;

use crate::re::Re;

/// Plain keywords, checked in this order against the lower-cased title.
const KEYWORDS: &[(&[&str], Language)] = &[
    (&["english"], Language::ENGLISH),
    (&["spanish"], Language::SPANISH),
    (&["danish"], Language::DANISH),
    (&["dutch"], Language::DUTCH),
    (&["japanese"], Language::JAPANESE),
    (&["icelandic"], Language::ICELANDIC),
    (&["mandarin", "cantonese", "chinese"], Language::CHINESE),
    (&["korean"], Language::KOREAN),
    (&["russian"], Language::RUSSIAN),
    (&["romanian"], Language::ROMANIAN),
    (&["hindi"], Language::HINDI),
    (&["arabic"], Language::ARABIC),
    (&["thai"], Language::THAI),
    (&["bulgarian"], Language::BULGARIAN),
    (&["polish"], Language::POLISH),
    (&["vietnamese"], Language::VIETNAMESE),
    (&["swedish"], Language::SWEDISH),
    (&["norwegian"], Language::NORWEGIAN),
    (&["finnish"], Language::FINNISH),
    (&["turkish"], Language::TURKISH),
    (&["portuguese"], Language::PORTUGUESE),
    (&["brazilian"], Language::PORTUGUESE_BR),
    (&["hungarian"], Language::HUNGARIAN),
    (&["hebrew"], Language::HEBREW),
    (&["ukrainian"], Language::UKRAINIAN),
    (&["persian"], Language::PERSIAN),
    (&["bengali"], Language::BENGALI),
    (&["slovak"], Language::SLOVAK),
    (&["latvian"], Language::LATVIAN),
    (&["latino"], Language::SPANISH_LATINO),
    (&["tamil"], Language::TAMIL),
    (&["telugu"], Language::TELUGU),
    (&["malayalam"], Language::MALAYALAM),
    (&["kannada"], Language::KANNADA),
    (&["albanian"], Language::ALBANIAN),
    (&["afrikaans"], Language::AFRIKAANS),
    (&["marathi"], Language::MARATHI),
    (&["tagalog"], Language::TAGALOG),
];

/// Joins a multi-line pattern (drops indentation, like IgnorePatternWhitespace).
fn joined(lines: &[&str]) -> String {
    lines.iter().map(|l| l.trim()).collect()
}

static LANGUAGE: LazyLock<Re> = LazyLock::new(|| {
    Re::ci(&joined(&[
        r"(?:\W|_|^)(?<english>\beng\b)|",
        r"(?<italian>\b(?:ita|italian)\b)|",
        r"(?<german>(?:swiss)?german\b|videomann|ger[. ]dub|\bger\b)|",
        r"(?<flemish>flemish)|",
        r"(?<bulgarian>bgaudio)|",
        r"(?<romanian>rodubbed)|",
        r"(?<brazilian>\b(dublado|pt-BR)\b)|",
        r"(?<greek>greek)|",
        r"(?<french>\b(?:FR|VO|VF|VFF|VFQ|VFI|VF2|TRUEFRENCH|FRENCH|FRE|FRA)\b)|",
        r"(?<russian>\b(?:rus|ru)\b)|",
        r"(?<hungarian>\b(?:HUNDUB|HUN)\b)|",
        r"(?<hebrew>\b(?:HebDub|HebDubbed)\b)|",
        r"(?<polish>\b(?:PL\W?DUB|DUB\W?PL|LEK\W?PL|PL\W?LEK)\b)|",
        r"(?<chinese>\[(?:CH[ST]|BIG5|GB)\]|简|繁|字幕)|",
        r"(?<ukrainian>(?:(?:\dx)?UKR))|",
        r"(?<spanish>\b(?:español|castellano)\b)|",
        r"(?<catalan>\b(?:catalan?|catalán|català)\b)|",
        r"(?<latvian>\b(?:lat|lav|lv)\b)|",
        r"(?<telugu>\btel\b)|",
        r"(?<vietnamese>\bVIE\b)|",
        r"(?<japanese>\bJAP\b)|",
        r"(?<korean>\bKOR\b)|",
        r"(?<urdu>\burdu\b)|",
        r"(?<romansh>\b(?:romansh|rumantsch|romansch)\b)|",
        r"(?<mongolian>\b(?:mongolian|khalkha)\b)|",
        r"(?<georgian>\b(?:georgian|geo|ka|kat)\b)|",
        r"(?<original>\b(?:orig|original)\b)",
    ]))
});

// Two-letter codes are only trusted in upper case, hence case-sensitive.
static CASE_SENSITIVE_LANGUAGE: LazyLock<Re> = LazyLock::new(|| {
    Re::new(&joined(&[
        r"(?:(?i)(?<!SUB[\W|_|^]))(?:(?<english>\bEN\b)|",
        r"(?<lithuanian>\bLT\b)|",
        r"(?<czech>\bCZ\b)|",
        r"(?<polish>\bPL\b)|",
        r"(?<bulgarian>\bBG\b)|",
        r"(?<slovak>\bSK\b)|",
        r"(?<german>\bDE\b)|",
        r"(?<spanish>\b(?<!DTS[._ -])ES\b))(?:(?i)(?![\W|_|^]SUB))",
    ]))
});

static GERMAN_DUAL: LazyLock<Re> = LazyLock::new(|| Re::ci(r"(?<!WEB[-_. ]?)\bDL\b"));
static GERMAN_MULTI: LazyLock<Re> = LazyLock::new(|| Re::ci(r"\bML\b"));

/// Languages named in a release title. Returns `[Unknown]` when nothing is found.
pub fn parse_languages(title: &str) -> Vec<Language> {
    let lower = title.to_lowercase();
    let mut languages: Vec<Language> = Vec::new();

    for (words, language) in KEYWORDS {
        if words.iter().any(|w| lower.contains(w)) {
            languages.push(*language);
        }
    }

    for m in CASE_SENSITIVE_LANGUAGE.captures_all(title) {
        for (group, language) in [
            ("english", Language::ENGLISH),
            ("lithuanian", Language::LITHUANIAN),
            ("czech", Language::CZECH),
            ("polish", Language::POLISH),
            ("bulgarian", Language::BULGARIAN),
            ("slovak", Language::SLOVAK),
            ("spanish", Language::SPANISH),
            ("german", Language::GERMAN),
        ] {
            if m.has(group) {
                languages.push(language);
            }
        }
    }

    for m in LANGUAGE.captures_all(title) {
        for (group, language) in [
            ("english", Language::ENGLISH),
            ("italian", Language::ITALIAN),
            ("german", Language::GERMAN),
            ("flemish", Language::FLEMISH),
            ("greek", Language::GREEK),
            ("french", Language::FRENCH),
            ("russian", Language::RUSSIAN),
            ("bulgarian", Language::BULGARIAN),
            ("brazilian", Language::PORTUGUESE_BR),
            ("hungarian", Language::HUNGARIAN),
            ("hebrew", Language::HEBREW),
            ("polish", Language::POLISH),
            ("chinese", Language::CHINESE),
            ("spanish", Language::SPANISH),
            ("catalan", Language::CATALAN),
            ("ukrainian", Language::UKRAINIAN),
            ("latvian", Language::LATVIAN),
            ("romanian", Language::ROMANIAN),
            ("telugu", Language::TELUGU),
            ("vietnamese", Language::VIETNAMESE),
            ("japanese", Language::JAPANESE),
            ("korean", Language::KOREAN),
            ("urdu", Language::URDU),
            ("romansh", Language::ROMANSH),
            ("mongolian", Language::MONGOLIAN),
            ("georgian", Language::GEORGIAN),
            ("original", Language::ORIGINAL),
        ] {
            if m.has(group) {
                languages.push(language);
            }
        }
    }

    if languages.is_empty() {
        languages.push(Language::UNKNOWN);
    }

    // "German DL" / "German ML" releases carry the original audio too.
    if languages == [Language::GERMAN] {
        if GERMAN_DUAL.is_match(title) {
            languages.push(Language::ORIGINAL);
        } else if GERMAN_MULTI.is_match(title) {
            languages.push(Language::ORIGINAL);
            languages.push(Language::ENGLISH);
        }
    }

    // Distinct by id, keeping first occurrence.
    let mut seen = Vec::new();
    languages.retain(|l| {
        if seen.contains(&l.id) {
            false
        } else {
            seen.push(l.id);
            true
        }
    });
    languages
}
