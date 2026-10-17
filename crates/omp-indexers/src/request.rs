//! Builds the requests for RSS sync and movie search. Ported from Radarr
//! `NewznabRequestGenerator` (GPL-3.0).
//!
//! The result is a list of *tiers*. Each tier is tried in order and the first
//! tier that yields releases wins, so id searches (precise) come before text
//! searches (noisy).

use omp_parser::normalize::remove_accent;
use url::form_urlencoded::byte_serialize;

use crate::{caps::Capabilities, settings::IndexerSettings};

/// Radarr pages 100 at a time, up to 30 pages.
pub const PAGE_SIZE: usize = 100;
pub const MAX_PAGES: usize = 30;

/// One logical query. The client adds `offset`/`limit` for each page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestSpec {
    /// Everything up to and including the search type and categories, without paging.
    pub base_url: String,
    /// Extra parameters such as `&tmdbid=603`.
    pub params: String,
}

impl RequestSpec {
    pub fn page_url(&self, page: usize) -> String {
        format!(
            "{}&offset={}&limit={PAGE_SIZE}{}",
            self.base_url,
            page * PAGE_SIZE,
            self.params
        )
    }
}

/// What to search for.
#[derive(Debug, Clone, Default)]
pub struct MovieSearch {
    pub tmdb_id: i64,
    /// "tt0133093".
    pub imdb_id: Option<String>,
    pub year: i32,
    /// Titles to try: the movie's title and alternative titles.
    pub titles: Vec<String>,
}

fn spec(settings: &IndexerSettings, search_type: &str, params: &str) -> Option<RequestSpec> {
    if settings.categories.is_empty() {
        return None;
    }
    let mut seen = Vec::new();
    for c in &settings.categories {
        if !seen.contains(c) {
            seen.push(*c);
        }
    }
    let cats = seen
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut base = format!(
        "{}?t={search_type}&cat={cats}&extended=1{}",
        settings.api_url(),
        settings.additional_parameters
    );
    if !settings.api_key.trim().is_empty() {
        base.push_str("&apikey=");
        base.push_str(&settings.api_key);
    }
    Some(RequestSpec {
        base_url: base,
        params: params.to_owned(),
    })
}

/// The recent-releases feed. A movie search is used when the indexer has one
/// (some forget to enable it, in which case plain search works).
pub fn generate_recent_requests(
    settings: &IndexerSettings,
    caps: &Capabilities,
) -> Vec<Vec<RequestSpec>> {
    let search_type = if caps.movie_params.is_some() {
        "movie"
    } else if caps.search_params.is_some() {
        "search"
    } else {
        return vec![];
    };
    vec![spec(settings, search_type, "").into_iter().collect()]
}

/// Makes a title safe for a query: no leading "The", "&" to "and", quotes and
/// dots dropped, other punctuation to `+`, accents removed.
pub fn clean_scene_title(title: &str) -> String {
    // Only a real leading "The " (case-insensitive) goes; "Theater" stays.
    let mut chars = title.chars();
    let starts_with_the = chars
        .by_ref()
        .take(3)
        .collect::<String>()
        .eq_ignore_ascii_case("the")
        && chars.next().is_some_and(char::is_whitespace);
    let without_the = if starts_with_the {
        title.char_indices().nth(4).map_or("", |(i, _)| &title[i..])
    } else {
        title
    };
    let replaced = without_the.replace('&', "and");
    let dropped: String = replaced
        .chars()
        .filter(|c| !matches!(c, '\'' | '.' | '`' | '\u{00B4}' | '\u{2018}' | '\u{2019}'))
        .collect();
    let plus: String = dropped
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '+'
            }
        })
        .collect();
    let mut collapsed = String::with_capacity(plus.len());
    for c in plus.chars() {
        if c == '+' && collapsed.ends_with('+') {
            continue;
        }
        collapsed.push(c);
    }
    remove_accent(&collapsed)
        .trim_matches(['+', ' '])
        .to_owned()
}

/// Percent-encodes a query title. '+' becomes a space first, like Radarr.
fn newznabify(title: &str) -> String {
    let spaced = title.replace('+', " ");
    byte_serialize(spaced.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

pub fn generate_movie_requests(
    settings: &IndexerSettings,
    caps: &Capabilities,
    search: &MovieSearch,
) -> Vec<Vec<RequestSpec>> {
    let mut tiers: Vec<Vec<RequestSpec>> = Vec::new();
    let mut ids_tier: Vec<RequestSpec> = Vec::new();

    let use_tmdb = caps.supports_tmdb_search() && search.tmdb_id > 0;
    let imdb = search
        .imdb_id
        .as_deref()
        .filter(|i| i.len() > 2)
        .map(|i| &i[2..]);
    let use_imdb = caps.supports_imdb_search() && imdb.is_some();

    if caps.supports_aggregate_id_search && (use_tmdb || use_imdb) {
        let mut ids = String::new();
        if use_tmdb {
            ids.push_str(&format!("&tmdbid={}", search.tmdb_id));
        }
        if let (true, Some(imdb)) = (use_imdb, imdb) {
            ids.push_str(&format!("&imdbid={imdb}"));
        }
        ids_tier.extend(spec(settings, "movie", &ids));
    } else if use_tmdb {
        ids_tier.extend(spec(
            settings,
            "movie",
            &format!("&tmdbid={}", search.tmdb_id),
        ));
    } else if let (true, Some(imdb)) = (use_imdb, imdb) {
        ids_tier.extend(spec(settings, "movie", &format!("&imdbid={imdb}")));
    }
    // Radarr always has a first tier, even when it is empty.
    tiers.push(ids_tier);

    if caps.supports_text_search() && search.year > 0 {
        let mut text_tier = Vec::new();
        let mut titles: Vec<String> = if caps.text_search_engine == "raw" {
            search.titles.clone()
        } else {
            let mut cleaned: Vec<String> = Vec::new();
            for t in &search.titles {
                let c = clean_scene_title(t);
                if !cleaned.contains(&c) {
                    cleaned.push(c);
                }
            }
            cleaned
        };
        titles.retain(|t| !t.is_empty());
        for title in titles {
            let mut query = title;
            if !settings.remove_year {
                query.push_str(&format!(" {}", search.year));
            }
            text_tier.extend(spec(
                settings,
                "search",
                &format!("&q={}", newznabify(&query)),
            ));
        }
        tiers.push(text_tier);
    }
    tiers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> IndexerSettings {
        IndexerSettings {
            base_url: "https://idx.example".into(),
            api_key: "KEY".into(),
            categories: vec![2000, 2040, 2000],
            ..Default::default()
        }
    }

    fn caps_with(movie: &[&str], search: &[&str]) -> Capabilities {
        Capabilities {
            movie_params: Some(movie.iter().map(|s| s.to_string()).collect()),
            search_params: Some(search.iter().map(|s| s.to_string()).collect()),
            supports_aggregate_id_search: true,
            ..Default::default()
        }
    }

    fn search() -> MovieSearch {
        MovieSearch {
            tmdb_id: 603,
            imdb_id: Some("tt0133093".into()),
            year: 1999,
            titles: vec!["The Matrix".into(), "Matrix, The".into()],
        }
    }

    #[test]
    fn title_cleaning() {
        assert_eq!(clean_scene_title("The Matrix"), "Matrix");
        assert_eq!(clean_scene_title("Tom & Jerry"), "Tom+and+Jerry");
        assert_eq!(
            clean_scene_title("L'Auberge Espagnole"),
            "LAuberge+Espagnole"
        );
        assert_eq!(clean_scene_title("Amélie: The Movie!"), "Amelie+The+Movie");
        assert_eq!(clean_scene_title("  Spaced   Out  "), "Spaced+Out");
        assert_eq!(
            clean_scene_title("Theater"),
            "Theater",
            "'The' only as a word"
        );
    }

    #[test]
    fn recent_uses_movie_search_when_available() {
        let r = generate_recent_requests(&settings(), &caps_with(&["q"], &["q"]));
        assert_eq!(r.len(), 1);
        let url = r[0][0].page_url(0);
        assert_eq!(
            url,
            "https://idx.example/api?t=movie&cat=2000,2040&extended=1&apikey=KEY&offset=0&limit=100"
        );
        let mut caps = caps_with(&[], &["q"]);
        caps.movie_params = None;
        let r = generate_recent_requests(&settings(), &caps);
        assert!(r[0][0].base_url.contains("t=search"));
        caps.search_params = None;
        assert!(generate_recent_requests(&settings(), &caps).is_empty());
    }

    #[test]
    fn pages_advance_by_the_page_size() {
        let r = generate_recent_requests(&settings(), &caps_with(&["q"], &["q"]));
        assert!(r[0][0].page_url(2).ends_with("&offset=200&limit=100"));
    }

    #[test]
    fn aggregate_id_search_sends_both_ids_in_one_request() {
        let tiers = generate_movie_requests(
            &settings(),
            &caps_with(&["q", "tmdbid", "imdbid"], &["q"]),
            &search(),
        );
        assert_eq!(tiers[0].len(), 1);
        assert_eq!(tiers[0][0].params, "&tmdbid=603&imdbid=0133093");
    }

    #[test]
    fn without_aggregate_support_tmdb_wins_over_imdb() {
        let mut caps = caps_with(&["q", "tmdbid", "imdbid"], &["q"]);
        caps.supports_aggregate_id_search = false;
        let tiers = generate_movie_requests(&settings(), &caps, &search());
        assert_eq!(tiers[0].len(), 1);
        assert_eq!(tiers[0][0].params, "&tmdbid=603");
        let mut only_imdb = caps_with(&["q", "imdbid"], &["q"]);
        only_imdb.supports_aggregate_id_search = false;
        let tiers = generate_movie_requests(&settings(), &only_imdb, &search());
        assert_eq!(tiers[0][0].params, "&imdbid=0133093");
    }

    #[test]
    fn text_search_is_the_second_tier_with_cleaned_deduplicated_titles() {
        let tiers =
            generate_movie_requests(&settings(), &caps_with(&["q", "tmdbid"], &["q"]), &search());
        assert_eq!(tiers.len(), 2);
        // "The Matrix" cleans to "Matrix"; "Matrix, The" to "Matrix+The".
        let queries: Vec<_> = tiers[1].iter().map(|s| s.params.clone()).collect();
        assert_eq!(queries, ["&q=Matrix%201999", "&q=Matrix%20The%201999"]);
        assert!(tiers[1][0].base_url.contains("t=search"));
    }

    #[test]
    fn raw_search_engines_get_the_titles_unchanged() {
        let mut caps = caps_with(&["q"], &["q"]);
        caps.text_search_engine = "raw".into();
        let tiers = generate_movie_requests(&settings(), &caps, &search());
        assert_eq!(tiers[1][0].params, "&q=The%20Matrix%201999");
    }

    #[test]
    fn remove_year_and_missing_year() {
        let mut s = settings();
        s.remove_year = true;
        let tiers = generate_movie_requests(&s, &caps_with(&["q"], &["q"]), &search());
        assert_eq!(tiers[1][0].params, "&q=Matrix");
        let mut no_year = search();
        no_year.year = 0;
        let tiers = generate_movie_requests(&settings(), &caps_with(&["q"], &["q"]), &no_year);
        assert_eq!(tiers.len(), 1, "no text tier without a year");
        assert!(tiers[0].is_empty(), "and no ids supported either");
    }

    #[test]
    fn no_categories_means_no_requests() {
        let mut s = settings();
        s.categories.clear();
        assert!(generate_recent_requests(&s, &caps_with(&["q"], &["q"]))[0].is_empty());
    }

    #[test]
    fn additional_parameters_and_missing_key() {
        let mut s = settings();
        s.api_key.clear();
        s.additional_parameters = "&maxage=500".into();
        let r = generate_recent_requests(&s, &caps_with(&["q"], &["q"]));
        assert_eq!(
            r[0][0].base_url,
            "https://idx.example/api?t=movie&cat=2000,2040&extended=1&maxage=500"
        );
    }
}
