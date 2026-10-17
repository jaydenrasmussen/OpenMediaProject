//! Release profiles: terms a release must or must not contain.
//!
//! A term is plain text (case-insensitive "contains") or a Perl-style regex
//! written `/pattern/flags`, as in Radarr.

use omp_parser::re::Re;

#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseProfile {
    pub id: i32,
    pub name: String,
    pub enabled: bool,
    /// A release must contain at least one of these (when the list is non-empty).
    pub required: Vec<String>,
    /// A release must contain none of these.
    pub ignored: Vec<String>,
    /// 0 means every indexer.
    pub indexer_id: i32,
    /// Empty means every movie.
    pub tags: Vec<i32>,
}

impl ReleaseProfile {
    /// Enabled profiles that apply to a movie with `tags` and an indexer.
    pub fn enabled_for<'a>(
        profiles: &'a [ReleaseProfile],
        tags: &[i32],
        indexer_id: i32,
    ) -> Vec<&'a ReleaseProfile> {
        profiles
            .iter()
            .filter(|p| p.tags.is_empty() || p.tags.iter().any(|t| tags.contains(t)))
            .filter(|p| p.enabled)
            .filter(|p| p.indexer_id == 0 || p.indexer_id == indexer_id)
            .collect()
    }
}

/// Does `term` match `text`? Errors when a `/regex/flags` term is invalid.
pub fn term_matches(term: &str, text: &str) -> Result<bool, String> {
    match perl_regex(term)? {
        Some(re) => Ok(re.is_match(text)),
        None => Ok(text.to_lowercase().contains(&term.to_lowercase())),
    }
}

/// Parses `/pattern/flags`. `Ok(None)` when `term` is not in that form.
fn perl_regex(term: &str) -> Result<Option<Re>, String> {
    // Same shape as Radarr: the form can appear anywhere, pattern is greedy.
    let Some(first) = term.find('/') else {
        return Ok(None);
    };
    let Some(last) = term.rfind('/').filter(|l| *l > first) else {
        return Ok(None);
    };
    let pattern = &term[first + 1..last];
    let after = &term[last + 1..];
    let flags: String = after
        .chars()
        .take_while(|c| c.is_ascii_lowercase())
        .collect();

    let mut inline = String::new();
    let mut case_insensitive = false;
    for flag in flags.chars() {
        match flag {
            'i' => case_insensitive = true,
            'm' | 's' | 'x' => inline.push(flag),
            'n' => {}
            other => {
                return Err(format!(
                    "Unknown or unsupported perl regex modifier: {other}"
                ));
            }
        }
    }
    let full = if inline.is_empty() {
        pattern.to_owned()
    } else {
        format!("(?{inline}){pattern}")
    };
    if case_insensitive {
        Re::try_ci(&full).map(Some)
    } else {
        Re::try_new(&full).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_terms_are_case_insensitive_contains() {
        assert_eq!(term_matches("remux", "Movie.2020.REMUX.1080p"), Ok(true));
        assert_eq!(term_matches("remux", "Movie.2020.WEB-DL"), Ok(false));
    }

    #[test]
    fn regex_terms_honour_flags() {
        assert_eq!(term_matches(r"/x26[45]/i", "Movie X265"), Ok(true));
        assert_eq!(
            term_matches(r"/x26[45]/", "Movie X265"),
            Ok(false),
            "no i flag"
        );
        assert_eq!(
            term_matches(r"/^Movie\.2020/", "Movie.2020.1080p"),
            Ok(true)
        );
    }

    #[test]
    fn bad_regex_or_flag_is_an_error() {
        assert!(term_matches("/(oops/i", "x").is_err());
        assert!(term_matches("/x/q", "x").is_err());
    }

    #[test]
    fn a_single_slash_is_plain_text() {
        assert_eq!(term_matches("AC/DC", "Movie AC/DC Live"), Ok(true));
        assert_eq!(term_matches("AC/DC", "Movie ACDC Live"), Ok(false));
    }

    fn rp(enabled: bool, tags: &[i32], indexer: i32) -> ReleaseProfile {
        ReleaseProfile {
            id: 1,
            name: "p".into(),
            enabled,
            required: vec![],
            ignored: vec![],
            indexer_id: indexer,
            tags: tags.to_vec(),
        }
    }

    #[test]
    fn selection_by_tag_indexer_and_enabled() {
        let all = vec![
            rp(true, &[], 0),
            rp(true, &[5], 0),
            rp(false, &[], 0),
            rp(true, &[], 9),
        ];
        assert_eq!(ReleaseProfile::enabled_for(&all, &[], 1).len(), 1);
        assert_eq!(ReleaseProfile::enabled_for(&all, &[5], 1).len(), 2);
        assert_eq!(ReleaseProfile::enabled_for(&all, &[], 9).len(), 2);
    }
}
