//! Taking raw releases from an indexer through parsing, library matching,
//! custom-format scoring and the decision engine.

use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Utc};
use omp_core::{language::Language, library::ExistingFile, release::ReleaseInfo};
use omp_db::{Db, DbError, repo::MovieRecord};
use omp_decision::{
    Decision, DecisionEngine, DecisionInfo, ReleaseSource, SearchContext, Subject,
    compare::sort_best_first,
    reason::{Rejection, RejectionKind, RejectionReason},
};
use omp_library::{Library, LibraryError};
use omp_parser::title::{ParsedMovieInfo, parse_movie_title};
use omp_quality::profile::QualityProfile;

use crate::{
    DiskSpace, QueueMap,
    config::load_decision_config,
    formats::{input_for_file, input_for_release, matching_ids},
    snapshot::DecisionSnapshot,
};

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("stored settings are unreadable: {0}")]
    Settings(#[from] serde_json::Error),
}

/// What to evaluate and in what context.
pub struct EvaluateRequest {
    pub releases: Vec<ReleaseInfo>,
    /// Set for a search aimed at one movie.
    pub search: Option<SearchContext>,
    /// True for a release pushed to us rather than found by us.
    pub pushed: bool,
    pub now: DateTime<Utc>,
    pub queue: QueueMap,
    pub disk: Arc<dyn DiskSpace>,
}

/// One release with everything learned about it.
#[derive(Debug, Clone)]
pub struct EvaluatedRelease {
    pub release: ReleaseInfo,
    pub parsed: Option<ParsedMovieInfo>,
    pub movie: Option<MovieRecord>,
    pub profile: Option<QualityProfile>,
    pub existing_file: Option<ExistingFile>,
    pub custom_format_ids: Vec<i32>,
    pub custom_format_score: i32,
    pub languages: Vec<Language>,
    pub source: ReleaseSource,
    pub decision: Decision,
}

impl EvaluatedRelease {
    /// Runs `f` with the engine's view of this release, if it was matched.
    pub fn with_subject<R>(&self, f: impl FnOnce(&Subject<'_>) -> R) -> Option<R> {
        let (parsed, movie, profile) = (
            self.parsed.as_ref()?,
            self.movie.as_ref()?,
            self.profile.as_ref()?,
        );
        let subject = Subject {
            release: &self.release,
            parsed,
            movie: &movie.movie,
            metadata: &movie.metadata,
            profile,
            existing_file: self.existing_file.as_ref(),
            custom_format_ids: &self.custom_format_ids,
            custom_format_score: self.custom_format_score,
            languages: &self.languages,
            source: self.source,
        };
        Some(f(&subject))
    }
}

pub struct Evaluation {
    /// In the order the releases came in.
    pub items: Vec<EvaluatedRelease>,
    pub snapshot: DecisionSnapshot,
}

impl Evaluation {
    /// Releases grouped by movie, best first within each group. Groups come in
    /// the order their first release appeared. `include` picks which releases
    /// take part (only matched ones can).
    fn ranked_by_movie(&self, include: impl Fn(&EvaluatedRelease) -> bool) -> Vec<Vec<usize>> {
        let mut groups: Vec<(i64, Vec<usize>)> = Vec::new();
        for (i, item) in self.items.iter().enumerate() {
            let Some(movie) = item.movie.as_ref().filter(|_| include(item)) else {
                continue;
            };
            match groups.iter_mut().find(|(id, _)| *id == movie.movie.id) {
                Some((_, v)) => v.push(i),
                None => groups.push((movie.movie.id, vec![i])),
            }
        }
        groups
            .into_iter()
            .map(|(_, indices)| self.rank(&indices))
            .collect()
    }

    /// Sorts item indices best first with the decision engine's comparer.
    fn rank(&self, indices: &[usize]) -> Vec<usize> {
        let subjects: Vec<(usize, Subject<'_>)> = indices
            .iter()
            .filter_map(|&i| {
                let it = &self.items[i];
                Some((
                    i,
                    Subject {
                        release: &it.release,
                        parsed: it.parsed.as_ref()?,
                        movie: &it.movie.as_ref()?.movie,
                        metadata: &it.movie.as_ref()?.metadata,
                        profile: it.profile.as_ref()?,
                        existing_file: it.existing_file.as_ref(),
                        custom_format_ids: &it.custom_format_ids,
                        custom_format_score: it.custom_format_score,
                        languages: &it.languages,
                        source: it.source,
                    },
                ))
            })
            .collect();
        let mut refs: Vec<&Subject<'_>> = subjects.iter().map(|(_, s)| s).collect();
        sort_best_first(&mut refs, &self.snapshot);
        refs.iter()
            .filter_map(|r| {
                subjects
                    .iter()
                    .find(|(_, s)| std::ptr::eq(s, *r))
                    .map(|(i, _)| *i)
            })
            .collect()
    }

    /// Approved releases, grouped by movie and best first within each group.
    pub fn approved_best_first(&self) -> Vec<Vec<usize>> {
        self.ranked_by_movie(|it| it.decision.approved())
    }

    /// Approved and temporarily rejected releases, ranked per movie. These are
    /// the ones worth grabbing now or holding for later.
    pub fn qualified_best_first(&self) -> Vec<usize> {
        self.ranked_by_movie(|it| it.decision.approved() || it.decision.temporarily_rejected())
            .into_iter()
            .flatten()
            .collect()
    }
}

fn rejected(reason: RejectionReason, message: impl Into<String>) -> Decision {
    Decision {
        rejections: vec![Rejection {
            reason,
            message: message.into(),
            kind: RejectionKind::Permanent,
        }],
    }
}

fn release_source(search: Option<SearchContext>, pushed: bool) -> ReleaseSource {
    match search {
        None if pushed => ReleaseSource::ReleasePush,
        None => ReleaseSource::Rss,
        Some(s) if s.interactive => ReleaseSource::InteractiveSearch,
        Some(s) if s.user_invoked => ReleaseSource::UserInvokedSearch,
        Some(_) => ReleaseSource::Search,
    }
}

/// Evaluates a batch of releases. RSS-style evaluation (no search) silently
/// drops releases it cannot parse; a search reports them as rejections.
pub async fn evaluate_releases(
    db: &Db,
    engine: &DecisionEngine,
    request: EvaluateRequest,
) -> Result<Evaluation, ServiceError> {
    let EvaluateRequest {
        releases,
        search,
        pushed,
        now,
        queue,
        disk,
    } = request;
    let source = release_source(search, pushed);
    let info = DecisionInfo {
        pushed_release: pushed,
        search,
    };

    let formats = db.custom_formats().load_compiled().await?;
    let profiles: HashMap<i32, QualityProfile> = db
        .quality_profiles()
        .list()
        .await?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    let library = Library::new(db);
    let target = match search {
        Some(s) => Some(db.movies().get(s.movie_id).await?),
        None => None,
    };

    // Phase 1: parse and match, which needs the database.
    struct Matched {
        release: ReleaseInfo,
        parsed: Option<ParsedMovieInfo>,
        movie: Option<MovieRecord>,
        early: Option<Decision>,
    }
    let mut matched: Vec<Matched> = Vec::with_capacity(releases.len());
    for release in releases {
        let parsed = parse_movie_title(&release.title, false)
            .filter(|p| p.primary_title().is_some_and(|t| !t.trim().is_empty()));
        let Some(parsed) = parsed else {
            if search.is_some() {
                matched.push(Matched {
                    release,
                    parsed: None,
                    movie: None,
                    early: Some(rejected(
                        RejectionReason::UnableToParse,
                        "Unable to parse release",
                    )),
                });
            }
            continue;
        };
        match library
            .match_release(&parsed, release.imdb_id.as_deref(), i64::from(release.tmdb_id), target.as_ref())
            .await
        {
            Ok(Some(m)) => matched.push(Matched {
                release,
                parsed: Some(parsed),
                movie: Some(m.movie),
                early: None,
            }),
            Ok(None) => matched.push(Matched {
                release,
                parsed: Some(parsed),
                movie: None,
                early: Some(rejected(
                    RejectionReason::UnknownMovie,
                    if pushed {
                        "Unknown Movie. Unable to match to an existing movie; pushed releases are only matched to your library"
                    } else {
                        "Unknown Movie. Unable to match to an existing movie in your library"
                    },
                )),
            }),
            Err(LibraryError::Multiple(titles)) => matched.push(Matched {
                release,
                parsed: Some(parsed),
                movie: None,
                early: Some(rejected(
                    RejectionReason::Error,
                    format!("More than one movie matches this release: {}", titles.join(", ")),
                )),
            }),
            Err(LibraryError::Db(e)) => return Err(e.into()),
        }
    }

    // Phase 2: load what the engine needs for the movies involved.
    let mut movies: Vec<MovieRecord> = Vec::new();
    for m in &matched {
        if let Some(movie) = &m.movie
            && !movies.iter().any(|x| x.movie.id == movie.movie.id)
        {
            movies.push(movie.clone());
        }
    }
    let config = load_decision_config(db).await?;
    let snapshot = DecisionSnapshot::load(db, config, now, &movies, &formats, queue, disk).await?;

    let mut existing: HashMap<i64, Option<ExistingFile>> = HashMap::new();
    for movie in &movies {
        let file = db.movie_files().for_movie(movie.movie.id).await?;
        let converted = file.map(|f| {
            let ids = matching_ids(&formats, &input_for_file(&f, movie));
            f.to_existing_file(ids)
        });
        existing.insert(movie.movie.id, converted);
    }

    // Phase 3: score and decide. No I/O from here on.
    let mut items = Vec::with_capacity(matched.len());
    for m in matched {
        let profile = m
            .movie
            .as_ref()
            .and_then(|mv| profiles.get(&mv.movie.quality_profile_id))
            .cloned();
        let (mut ids, mut score, mut languages) = (Vec::new(), 0, Vec::new());
        if let (Some(parsed), Some(movie), Some(profile)) = (&m.parsed, &m.movie, &profile) {
            ids = matching_ids(&formats, &input_for_release(parsed, &m.release, movie));
            score = profile.calculate_custom_format_score(&ids);
            languages = parsed.languages.clone();
        }
        let existing_file = m
            .movie
            .as_ref()
            .and_then(|mv| existing.get(&mv.movie.id).cloned().flatten());

        let mut item = EvaluatedRelease {
            release: m.release,
            parsed: m.parsed,
            movie: m.movie,
            profile,
            existing_file,
            custom_format_ids: ids,
            custom_format_score: score,
            languages,
            source,
            decision: Decision::default(),
        };
        item.decision = match m.early {
            Some(early) => early,
            None => item
                .with_subject(|subject| engine.decide(subject, &info, &snapshot))
                .unwrap_or_else(|| {
                    rejected(
                        RejectionReason::Error,
                        "Unexpected error processing release",
                    )
                }),
        };
        items.push(item);
    }

    Ok(Evaluation { items, snapshot })
}
