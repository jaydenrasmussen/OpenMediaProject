//! Requests for TV: the recent feed and the searches. Ported from Sonarr
//! `NewznabRequestGenerator` (GPL-3.0).
//!
//! As for movies, the result is a list of tiers; the first tier that yields
//! releases wins. Id searches come first, text searches second.

use chrono::{Datelike, NaiveDate};
use omp_core::tv::SeriesType;

use crate::{
    caps::Capabilities,
    request::{RequestSpec, clean_scene_title, newznabify, spec_with},
    settings::IndexerSettings,
};

/// The series a search is for.
#[derive(Debug, Clone, Default)]
pub struct TvSeries {
    pub tvdb_id: Option<i64>,
    pub imdb_id: Option<String>,
    pub tmdb_id: i64,
    pub series_type: SeriesType,
    /// The titles to search by: the series' own and its alternative titles.
    pub titles: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum TvSearch {
    Episode {
        series: TvSeries,
        season: i32,
        episode: i32,
    },
    Season {
        series: TvSeries,
        season: i32,
    },
    DailyEpisode {
        series: TvSeries,
        air_date: NaiveDate,
    },
    DailySeason {
        series: TvSeries,
        year: i32,
    },
    AnimeEpisode {
        series: TvSeries,
        absolute: i32,
        season: i32,
        episode: i32,
    },
    AnimeSeason {
        series: TvSeries,
        season: i32,
        /// Titles that already name this season, searched without a season number.
        season_titles: Vec<String>,
    },
    /// An episode with no usable number, found by its title.
    Special {
        series: TvSeries,
        episode_titles: Vec<String>,
    },
}

/// Some indexers read season 0 as "no season".
fn season_param(n: i32) -> String {
    if n == 0 { "00".into() } else { n.to_string() }
}

fn tv_categories(settings: &IndexerSettings, series_type: SeriesType) -> &[i32] {
    if series_type == SeriesType::Anime {
        &settings.anime_categories
    } else {
        &settings.tv_categories
    }
}

/// The recent feed covers the TV and anime categories together.
pub fn generate_recent_tv_requests(
    settings: &IndexerSettings,
    caps: &Capabilities,
) -> Vec<Vec<RequestSpec>> {
    let mut all: Vec<i32> = settings.tv_categories.clone();
    all.extend(&settings.anime_categories);
    if caps.tv_params.is_some() {
        vec![
            spec_with(settings, &all, "tvsearch", "")
                .into_iter()
                .collect(),
        ]
    } else if caps.search_params.is_some() {
        vec![
            spec_with(settings, &settings.anime_categories, "search", "")
                .into_iter()
                .collect(),
        ]
    } else {
        vec![]
    }
}

fn id_requests(
    settings: &IndexerSettings,
    caps: &Capabilities,
    categories: &[i32],
    series: &TvSeries,
    params: &str,
) -> Vec<RequestSpec> {
    let tvdb = series
        .tvdb_id
        .filter(|id| *id > 0)
        .filter(|_| caps.tv_supports_tvdb());
    let imdb = series
        .imdb_id
        .as_deref()
        .filter(|i| !i.trim().is_empty())
        .filter(|_| caps.tv_supports_imdb());
    let tmdb = Some(series.tmdb_id)
        .filter(|id| *id > 0)
        .filter(|_| caps.tv_supports_tmdb());

    let mut out = Vec::new();
    if caps.supports_aggregate_id_search && (tvdb.is_some() || tmdb.is_some()) {
        let mut ids = String::new();
        if let Some(id) = tvdb {
            ids.push_str(&format!("&tvdbid={id}"));
        }
        if let Some(id) = imdb {
            ids.push_str(&format!("&imdbid={id}"));
        }
        if let Some(id) = tmdb {
            ids.push_str(&format!("&tmdbid={id}"));
        }
        out.extend(spec_with(
            settings,
            categories,
            "tvsearch",
            &format!("{ids}{params}"),
        ));
    } else if let Some(id) = tvdb {
        out.extend(spec_with(
            settings,
            categories,
            "tvsearch",
            &format!("&tvdbid={id}{params}"),
        ));
    } else if let Some(id) = imdb {
        out.extend(spec_with(
            settings,
            categories,
            "tvsearch",
            &format!("&imdbid={id}{params}"),
        ));
    } else if let Some(id) = tmdb {
        out.extend(spec_with(
            settings,
            categories,
            "tvsearch",
            &format!("&tmdbid={id}{params}"),
        ));
    }
    out
}

/// Search terms as the indexer wants them: untouched for "raw" engines,
/// otherwise scene-cleaned and de-duplicated.
fn query_titles(titles: &[String], engine: &str) -> Vec<String> {
    if engine == "raw" {
        return titles.to_vec();
    }
    let mut cleaned: Vec<String> = Vec::new();
    for t in titles {
        let c = clean_scene_title(t);
        if !c.is_empty() && !cleaned.contains(&c) {
            cleaned.push(c);
        }
    }
    cleaned
}

fn title_requests(
    settings: &IndexerSettings,
    caps: &Capabilities,
    categories: &[i32],
    series: &TvSeries,
    params: &str,
) -> Vec<RequestSpec> {
    let mut out = Vec::new();
    if caps.tv_supports_title() {
        for title in &series.titles {
            let escaped = urlencoding_escape(title);
            out.extend(spec_with(
                settings,
                categories,
                "tvsearch",
                &format!("&title={escaped}{params}"),
            ));
        }
    } else if caps.tv_supports_q() {
        for title in query_titles(&series.titles, &caps.tv_text_search_engine) {
            out.extend(spec_with(
                settings,
                categories,
                "tvsearch",
                &format!("&q={}{params}", newznabify(&title)),
            ));
        }
    }
    out
}

/// `Uri.EscapeDataString`: everything but unreserved characters.
fn urlencoding_escape(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

/// Standard and daily shows: ids first, then the title.
fn standard(
    settings: &IndexerSettings,
    caps: &Capabilities,
    series: &TvSeries,
    params: &str,
) -> Vec<Vec<RequestSpec>> {
    let categories = tv_categories(settings, series.series_type);
    let ids = id_requests(settings, caps, categories, series, params);
    let titles = title_requests(settings, caps, categories, series, params);
    vec![ids, titles]
}

fn has_search_inputs(caps: &Capabilities) -> bool {
    caps.tv_supports_q()
        || caps.tv_supports_title()
        || caps.tv_supports_tvdb()
        || caps.tv_supports_imdb()
        || caps.tv_supports_tvrage()
        || caps.tv_supports_tvmaze()
        || caps.tv_supports_tmdb()
}

pub fn generate_tv_requests(
    settings: &IndexerSettings,
    caps: &Capabilities,
    search: &TvSearch,
) -> Vec<Vec<RequestSpec>> {
    match search {
        TvSearch::Episode {
            series,
            season,
            episode,
        } => {
            if !caps.tv_supports_episode() || !has_search_inputs(caps) {
                return vec![];
            }
            standard(
                settings,
                caps,
                series,
                &format!("&season={}&ep={episode}", season_param(*season)),
            )
        }
        TvSearch::Season { series, season } => {
            if !caps.tv_supports_season() || !has_search_inputs(caps) {
                return vec![];
            }
            standard(
                settings,
                caps,
                series,
                &format!("&season={}", season_param(*season)),
            )
        }
        TvSearch::DailyEpisode { series, air_date } => {
            if !caps.tv_supports_episode() || !has_search_inputs(caps) {
                return vec![];
            }
            standard(
                settings,
                caps,
                series,
                &format!(
                    "&season={}&ep={:02}/{:02}",
                    air_date.year(),
                    air_date.month(),
                    air_date.day()
                ),
            )
        }
        TvSearch::DailySeason { series, year } => {
            if !caps.tv_supports_episode() || !has_search_inputs(caps) {
                return vec![];
            }
            standard(settings, caps, series, &format!("&season={year}"))
        }
        TvSearch::AnimeEpisode {
            series,
            absolute,
            season,
            episode,
        } => anime_episode(settings, caps, series, *absolute, *season, *episode),
        TvSearch::AnimeSeason {
            series,
            season,
            season_titles,
        } => anime_season(settings, caps, series, *season, season_titles),
        TvSearch::Special {
            series,
            episode_titles,
        } => {
            if !caps.supports_text_search() {
                return vec![];
            }
            let categories = tv_categories(settings, series.series_type);
            vec![
                episode_titles
                    .iter()
                    .filter_map(|title| {
                        let query = title.replace('+', " ");
                        spec_with(
                            settings,
                            categories,
                            "search",
                            &format!("&q={}", urlencoding_escape(&query)),
                        )
                    })
                    .collect(),
            ]
        }
    }
}

fn anime_episode(
    settings: &IndexerSettings,
    caps: &Capabilities,
    series: &TvSeries,
    absolute: i32,
    season: i32,
    episode: i32,
) -> Vec<Vec<RequestSpec>> {
    if !caps.supports_text_search() {
        return vec![];
    }
    let categories = &settings.anime_categories;
    let standard_too = settings.anime_standard_format_search
        && season > 0
        && episode > 0
        && caps.tv_supports_episode();
    let mut tier: Vec<RequestSpec> = id_requests(
        settings,
        caps,
        categories,
        series,
        &format!("&q={absolute:02}"),
    );
    if standard_too {
        tier.extend(id_requests(
            settings,
            caps,
            categories,
            series,
            &format!("&season={}&ep={episode}", season_param(season)),
        ));
    }
    for title in query_titles(&series.titles, &caps.text_search_engine) {
        let q = newznabify(&title);
        tier.extend(spec_with(
            settings,
            categories,
            "search",
            &format!("&q={q}+{absolute:02}"),
        ));
        if standard_too {
            tier.extend(spec_with(
                settings,
                categories,
                "tvsearch",
                &format!("&q={q}&season={}&ep={episode}", season_param(season)),
            ));
        }
    }
    vec![tier]
}

fn anime_season(
    settings: &IndexerSettings,
    caps: &Capabilities,
    series: &TvSeries,
    season: i32,
    season_titles: &[String],
) -> Vec<Vec<RequestSpec>> {
    if !caps.supports_text_search() || season <= 0 {
        return vec![];
    }
    let categories = &settings.anime_categories;
    let mut tier: Vec<RequestSpec> = Vec::new();
    if settings.anime_standard_format_search {
        tier.extend(id_requests(
            settings,
            caps,
            categories,
            series,
            &format!("&season={}", season_param(season)),
        ));
    }
    let own_season_titles = query_titles(season_titles, &caps.text_search_engine);
    for title in query_titles(&series.titles, &caps.text_search_engine) {
        let q = newznabify(&title);
        if own_season_titles
            .iter()
            .any(|t| t.eq_ignore_ascii_case(&title))
        {
            // The title already names the season; no number needed.
            tier.extend(spec_with(
                settings,
                categories,
                "search",
                &format!("&q={q}"),
            ));
        } else if settings.anime_standard_format_search {
            tier.extend(spec_with(
                settings,
                categories,
                "tvsearch",
                &format!("&q={q}&season={}", season_param(season)),
            ));
        }
    }
    vec![tier]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> IndexerSettings {
        IndexerSettings {
            base_url: "http://127.0.0.1:1234/".into(),
            api_key: "abcd".into(),
            tv_categories: vec![1, 2],
            anime_categories: vec![3, 4],
            ..Default::default()
        }
    }

    fn series() -> TvSeries {
        TvSeries {
            tvdb_id: Some(20),
            imdb_id: Some("t40".into()),
            tmdb_id: 50,
            series_type: SeriesType::Standard,
            titles: vec!["Monkey Island".into()],
        }
    }

    fn anime() -> TvSeries {
        TvSeries {
            series_type: SeriesType::Anime,
            titles: vec!["Monkey+Island".into()],
            ..series()
        }
    }

    fn caps(tv: &[&str]) -> Capabilities {
        Capabilities {
            tv_params: Some(tv.iter().map(|s| s.to_string()).collect()),
            ..Default::default()
        }
    }

    fn urls(tiers: &[Vec<RequestSpec>]) -> Vec<Vec<String>> {
        tiers
            .iter()
            .map(|t| t.iter().map(|s| s.page_url(0)).collect())
            .collect()
    }

    fn single() -> TvSearch {
        TvSearch::Episode {
            series: series(),
            season: 1,
            episode: 2,
        }
    }

    #[test]
    fn the_feed_uses_tv_and_anime_categories_together() {
        let r = generate_recent_tv_requests(&settings(), &Capabilities::default());
        assert_eq!(r.len(), 1);
        assert!(
            r[0][0].page_url(0).contains("t=tvsearch&cat=1,2,3,4&"),
            "{}",
            r[0][0].page_url(0)
        );
        // Without TV search, plain search over the anime categories is the fallback.
        let mut c = Capabilities {
            tv_params: None,
            ..Default::default()
        };
        let r = generate_recent_tv_requests(&settings(), &c);
        assert!(r[0][0].page_url(0).contains("t=search&cat=3,4&"));
        c.search_params = None;
        assert!(generate_recent_tv_requests(&settings(), &c).is_empty());
    }

    #[test]
    fn a_standard_episode_search_has_an_id_tier_then_a_title_tier() {
        let tiers = generate_tv_requests(
            &settings(),
            &caps(&["q", "tvdbid", "season", "ep"]),
            &single(),
        );
        let u = urls(&tiers);
        assert_eq!(u.len(), 2);
        assert_eq!(u[0].len(), 1);
        assert!(u[0][0].contains("&tvdbid=20&season=1&ep=2"), "{}", u[0][0]);
        assert!(u[0][0].contains("&cat=1,2&"));
        assert!(
            u[1][0].contains("&q=Monkey%20Island&season=1&ep=2"),
            "{}",
            u[1][0]
        );
    }

    #[test]
    fn each_supported_id_is_used_alone_in_priority_order() {
        let mut c = caps(&["q", "tvdbid", "imdbid", "tmdbid", "season", "ep"]);
        c.supports_aggregate_id_search = false;
        let one =
            |c: &Capabilities| urls(&generate_tv_requests(&settings(), c, &single()))[0].clone();
        assert!(one(&c)[0].contains("&tvdbid=20&season"));
        c.tv_params = Some(
            ["q", "imdbid", "tmdbid", "season", "ep"]
                .map(String::from)
                .to_vec(),
        );
        assert!(one(&c)[0].contains("&imdbid=t40&season"));
        c.tv_params = Some(["q", "tmdbid", "season", "ep"].map(String::from).to_vec());
        assert!(one(&c)[0].contains("&tmdbid=50&season"));
        c.tv_params = Some(["q", "season", "ep"].map(String::from).to_vec());
        assert!(
            one(&c).is_empty(),
            "no usable id: the id tier is empty and only the title tier remains"
        );
    }

    #[test]
    fn aggregate_search_sends_every_supported_id_at_once() {
        let mut c = caps(&["q", "tvdbid", "imdbid", "tmdbid", "season", "ep"]);
        c.supports_aggregate_id_search = true;
        let u = urls(&generate_tv_requests(&settings(), &c, &single()));
        assert!(
            u[0][0].contains("&tvdbid=20&imdbid=t40&tmdbid=50&season=1&ep=2"),
            "{}",
            u[0][0]
        );
    }

    #[test]
    fn searches_need_what_the_indexer_can_do() {
        // No season and ep parameters: no episode search at all.
        assert!(generate_tv_requests(&settings(), &caps(&["q", "tvdbid"]), &single()).is_empty());
        // No q, title or ids: nothing to search by.
        assert!(generate_tv_requests(&settings(), &caps(&["season", "ep"]), &single()).is_empty());
        // A season search only needs "season".
        let s = TvSearch::Season {
            series: series(),
            season: 3,
        };
        let tiers = generate_tv_requests(&settings(), &caps(&["q", "season"]), &s);
        assert!(urls(&tiers)[1][0].contains("&q=Monkey%20Island&season=3"));
    }

    #[test]
    fn the_title_parameter_beats_q_when_supported() {
        let c = caps(&["q", "title", "season", "ep"]);
        let u = urls(&generate_tv_requests(&settings(), &c, &single()));
        assert!(
            u[1][0].contains("&title=Monkey%20Island&season=1&ep=2"),
            "{}",
            u[1][0]
        );
    }

    #[test]
    fn season_zero_is_sent_as_00() {
        let s = TvSearch::Season {
            series: series(),
            season: 0,
        };
        let u = urls(&generate_tv_requests(
            &settings(),
            &caps(&["q", "season"]),
            &s,
        ));
        assert!(u[1][0].ends_with("&season=00"), "{}", u[1][0]);
    }

    #[test]
    fn daily_shows_search_by_date() {
        let s = TvSearch::DailyEpisode {
            series: series(),
            air_date: NaiveDate::from_ymd_opt(2018, 4, 8).unwrap(),
        };
        let u = urls(&generate_tv_requests(
            &settings(),
            &caps(&["q", "tvdbid", "season", "ep"]),
            &s,
        ));
        assert!(
            u[0][0].contains("&tvdbid=20&season=2018&ep=04/08"),
            "{}",
            u[0][0]
        );
        let y = TvSearch::DailySeason {
            series: series(),
            year: 2018,
        };
        let u = urls(&generate_tv_requests(
            &settings(),
            &caps(&["q", "tvdbid", "season", "ep"]),
            &y,
        ));
        assert!(u[0][0].ends_with("&season=2018"));
    }

    #[test]
    fn anime_searches_by_absolute_number_in_anime_categories() {
        let s = TvSearch::AnimeEpisode {
            series: anime(),
            absolute: 100,
            season: 5,
            episode: 4,
        };
        let tiers = generate_tv_requests(&settings(), &Capabilities::default(), &s);
        let u = urls(&tiers);
        assert_eq!(u.len(), 1, "one tier");
        assert_eq!(
            u[0].len(),
            1,
            "default caps know no ids besides the standard ones"
        );
        assert!(u[0][0].contains("t=search&cat=3,4&"), "{}", u[0][0]);
        assert!(u[0][0].contains("&q=Monkey%20Island+100"), "{}", u[0][0]);
    }

    #[test]
    fn anime_standard_format_search_adds_season_and_episode_requests() {
        let mut st = settings();
        st.anime_standard_format_search = true;
        let mut c = caps(&["q", "tvdbid", "season", "ep"]);
        c.search_params = Some(vec!["q".into()]);
        let s = TvSearch::AnimeEpisode {
            series: anime(),
            absolute: 100,
            season: 5,
            episode: 4,
        };
        let u = urls(&generate_tv_requests(&st, &c, &s));
        assert_eq!(u.len(), 1);
        let all = &u[0];
        assert!(all[0].contains("&tvdbid=20&q=100"), "{}", all[0]);
        assert!(all[1].contains("&tvdbid=20&season=5&ep=4"), "{}", all[1]);
        assert!(all[2].contains("t=search") && all[2].contains("&q=Monkey%20Island+100"));
        assert!(
            all[3].contains("t=tvsearch") && all[3].contains("&q=Monkey%20Island&season=5&ep=4")
        );
    }

    #[test]
    fn anime_season_titles_are_searched_without_a_season_number() {
        let mut st = settings();
        st.anime_standard_format_search = true;
        let mut s = anime();
        s.titles = vec!["Monkey Island".into(), "Monkey Island Revenge".into()];
        let search = TvSearch::AnimeSeason {
            series: s,
            season: 3,
            season_titles: vec!["Monkey Island Revenge".into()],
        };
        let u = urls(&generate_tv_requests(
            &st,
            &caps(&["q", "season", "ep"]),
            &search,
        ));
        let all = &u[0];
        assert!(
            all.iter()
                .any(|p| p.contains("&q=Monkey%20Island&season=3")),
            "{all:?}"
        );
        assert!(
            all.iter().any(|p| p.contains("t=search&")
                && p.contains("&q=Monkey%20Island%20Revenge")
                && !p.contains("season=")),
            "{all:?}"
        );
        // Season 0 has no anime season search.
        let none = TvSearch::AnimeSeason {
            series: anime(),
            season: 0,
            season_titles: vec![],
        };
        assert!(generate_tv_requests(&st, &caps(&["q"]), &none).is_empty());
    }

    #[test]
    fn specials_are_found_by_episode_title() {
        let s = TvSearch::Special {
            series: series(),
            episode_titles: vec!["The+Christmas Special".into()],
        };
        let u = urls(&generate_tv_requests(&settings(), &caps(&["q"]), &s));
        assert!(u[0][0].contains("t=search&cat=1,2&"));
        assert!(
            u[0][0].ends_with("&q=The%20Christmas%20Special"),
            "{}",
            u[0][0]
        );
    }

    #[test]
    fn no_categories_means_no_requests() {
        let mut st = settings();
        st.tv_categories.clear();
        assert!(
            urls(&generate_tv_requests(
                &st,
                &caps(&["q", "tvdbid", "season", "ep"]),
                &single()
            ))
            .iter()
            .all(Vec::is_empty)
        );
    }
}
