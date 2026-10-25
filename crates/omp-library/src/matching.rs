use omp_core::movie::MovieMetadata;
use omp_db::{Db, DbError, repo::MovieRecord};
use omp_parser::{normalize::clean_movie_title, title::ParsedMovieInfo};

use crate::romans::ARABIC_ROMAN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchType {
    Id,
    Title,
}

#[derive(Debug, Clone)]
pub struct MovieMatch {
    pub movie: MovieRecord,
    pub match_type: MatchType,
}

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    /// More than one library movie fits the release; the caller should not guess.
    #[error("expected one movie, found {} matching: {}", .0.len(), .0.join(", "))]
    Multiple(Vec<String>),
    #[error(transparent)]
    Db(#[from] DbError),
}

fn lower_clean(title: &str) -> String {
    clean_movie_title(title).to_lowercase()
}

/// The titles to look up and the "other" spellings, from `FindByTitleCandidates`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TitleLookup {
    /// Cleaned titles plus their Roman and Arabic variants.
    pub lookup_titles: Vec<String>,
    /// Only the variants (not the cleaned titles themselves).
    pub other_titles: Vec<String>,
}

/// Radarr swaps every 1..=21 for its Roman form in one title and the reverse in
/// another. The reverse swap looks for upper-case numerals in an already
/// lower-cased title, so it never changes anything; kept for parity.
pub fn title_lookup(titles: &[String]) -> TitleLookup {
    let mut out = TitleLookup::default();
    for title in titles {
        let clean = lower_clean(title);
        let mut roman = clean.clone();
        let mut arabic = clean.clone();
        for (arabic_text, roman_upper, _) in ARABIC_ROMAN.iter() {
            roman = roman.replace(arabic_text.as_str(), roman_upper);
            arabic = arabic.replace(roman_upper.as_str(), arabic_text);
        }
        let roman = roman.to_lowercase();
        out.other_titles.push(arabic.clone());
        out.other_titles.push(roman.clone());
        out.lookup_titles.push(clean);
        out.lookup_titles.push(arabic);
        out.lookup_titles.push(roman);
    }
    out
}

fn year_matches(metadata: &MovieMetadata, year: Option<i32>) -> bool {
    match year {
        None => true,
        Some(y) => metadata.year == y || metadata.secondary_year == Some(y),
    }
}

fn clean_original(metadata: &MovieMetadata) -> Option<String> {
    (!metadata.original_title.trim().is_empty()).then(|| lower_clean(&metadata.original_title))
}

/// Picks the movie for a set of parsed titles from candidates already fetched.
///
/// Order: main or original title, then the numeral variants, then alternative
/// titles. Each step applies the year if one is given. Two or more movies
/// at one step is an error.
pub fn find_by_title<'a>(
    candidates: &'a [MovieRecord],
    titles: &[String],
    year: Option<i32>,
    other_titles: &[String],
) -> Result<Option<&'a MovieRecord>, LibraryError> {
    let clean_titles: Vec<String> = titles.iter().map(|t| lower_clean(t)).collect();

    let pick = |matches: Vec<&'a MovieRecord>| -> Result<Option<&'a MovieRecord>, LibraryError> {
        match matches.as_slice() {
            [] => Ok(None),
            [one] => Ok(Some(one)),
            many => Err(LibraryError::Multiple(
                many.iter().map(|m| m.movie.title.clone()).collect(),
            )),
        }
    };
    let with_year = |it: &mut dyn Iterator<Item = &'a MovieRecord>| -> Vec<&'a MovieRecord> {
        it.filter(|m| year_matches(&m.metadata, year)).collect()
    };

    let step1 = with_year(&mut candidates.iter().filter(|m| {
        clean_titles.contains(&lower_clean(&m.metadata.title))
            || clean_original(&m.metadata).is_some_and(|o| clean_titles.contains(&o))
    }));
    if !step1.is_empty() {
        return pick(step1);
    }

    let step2 = with_year(
        &mut candidates
            .iter()
            .filter(|m| other_titles.contains(&lower_clean(&m.metadata.title))),
    );
    if !step2.is_empty() {
        return pick(step2);
    }

    let step3 = with_year(&mut candidates.iter().filter(|m| {
        m.metadata.alternative_titles.iter().any(|t| {
            let c = lower_clean(&t.title);
            clean_titles.contains(&c) || other_titles.contains(&c)
        })
    }));
    pick(step3)
}

/// Does a movie that the search was *for* fit the parsed titles, allowing
/// Roman/Arabic numeral differences?
fn titles_fit_target(parsed_titles: &[String], target: &MovieMetadata) -> bool {
    let mut possible: Vec<String> = vec![lower_clean(&target.title)];
    possible.extend(clean_original(target));
    possible.extend(
        target
            .alternative_titles
            .iter()
            .map(|t| lower_clean(&t.title)),
    );
    let clean: Vec<String> = parsed_titles.iter().map(|t| lower_clean(t)).collect();

    possible.iter().any(|pt| {
        clean.contains(pt)
            || ARABIC_ROMAN.iter().any(|(arabic, _, roman_lower)| {
                clean.contains(&pt.replace(arabic.as_str(), roman_lower))
                    || clean
                        .iter()
                        .any(|t| t.replace(arabic.as_str(), roman_lower) == *pt)
            })
    })
}

fn release_year_ok(parsed: &ParsedMovieInfo, metadata: &MovieMetadata) -> bool {
    parsed.year < 1800
        || metadata.year == parsed.year
        || metadata.secondary_year == Some(parsed.year)
}

/// Finds library movies. Wraps a database handle.
pub struct Library<'a> {
    db: &'a Db,
}

impl<'a> Library<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    async fn by_title_and_year(
        &self,
        parsed: &ParsedMovieInfo,
    ) -> Result<Option<MovieMatch>, LibraryError> {
        let lookup = title_lookup(&parsed.movie_titles);
        let candidates = self
            .db
            .movies()
            .find_by_any_clean_title(&lookup.lookup_titles)
            .await?;
        let year = (parsed.year > 1800).then_some(parsed.year);
        Ok(find_by_title(
            &candidates,
            &parsed.movie_titles,
            year,
            &lookup.other_titles,
        )?
        .map(|m| MovieMatch {
            movie: m.clone(),
            match_type: MatchType::Title,
        }))
    }

    /// Maps a parsed release to a library movie.
    ///
    /// Order: IMDb id, TMDB id (each must agree with the release year when it
    /// has one, because indexers often attach wrong ids), then title and year.
    /// With a `target` (a search for one movie), a title/id match against that
    /// movie is used instead of looking the whole library over.
    pub async fn match_release(
        &self,
        parsed: &ParsedMovieInfo,
        imdb_id: Option<&str>,
        tmdb_id: i64,
        target: Option<&MovieRecord>,
    ) -> Result<Option<MovieMatch>, LibraryError> {
        if let Some(imdb) = imdb_id.filter(|i| !i.trim().is_empty() && *i != "0")
            && let Some(m) = self.db.movies().find_by_imdb_id(imdb).await?
            && release_year_ok(parsed, &m.metadata)
        {
            return Ok(Some(MovieMatch {
                movie: m,
                match_type: MatchType::Id,
            }));
        }
        if tmdb_id > 0
            && let Some(m) = self.db.movies().find_by_tmdb_id(tmdb_id).await?
            && release_year_ok(parsed, &m.metadata)
        {
            return Ok(Some(MovieMatch {
                movie: m,
                match_type: MatchType::Id,
            }));
        }

        match target {
            None => self.by_title_and_year(parsed).await,
            Some(target) => {
                if titles_fit_target(&parsed.movie_titles, &target.metadata)
                    && release_year_ok(parsed, &target.metadata)
                {
                    return Ok(Some(MovieMatch {
                        movie: target.clone(),
                        match_type: MatchType::Title,
                    }));
                }
                let same_tmdb = tmdb_id > 0 && tmdb_id == target.movie.tmdb_id;
                let same_imdb = imdb_id.is_some_and(|i| {
                    !i.trim().is_empty() && target.metadata.imdb_id.as_deref() == Some(i)
                });
                if same_tmdb || same_imdb {
                    return Ok(Some(MovieMatch {
                        movie: target.clone(),
                        match_type: MatchType::Id,
                    }));
                }
                Ok(None)
            }
        }
    }
}
