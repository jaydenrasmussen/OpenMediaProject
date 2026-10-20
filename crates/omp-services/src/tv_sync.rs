//! RSS sync and searches for TV: fetch from the indexers, evaluate, grab.
//! Ported from Sonarr `RssSyncService` and the search services (GPL-3.0).

use std::sync::Arc;

use chrono::{DateTime, Datelike, Utc};
use omp_core::tv::SeriesType;
use omp_db::{
    Db,
    repo::{EpisodeRecord, ProviderKind, SeriesRecord},
};
use omp_decision::tv::{TvDecisionEngine, TvSearchContext, TvSearchKind};
use omp_downloads::ClientRegistry;
use omp_indexers::{LastSeen, TvSearch, TvSeries};

use crate::{
    DiskSpace,
    evaluate::ServiceError,
    grab::{GrabContext, ProcessedDecisions},
    sync::{IndexerFailure, IndexerPool, SearchMode, is_blocked, record_failure, same_release},
    tv_evaluate::{TvEvaluateRequest, TvEvaluation, TvQueueMap, evaluate_tv_releases},
    tv_grab::process_tv_decisions,
};

pub struct TvSyncContext {
    pub now: DateTime<Utc>,
    pub process_start: DateTime<Utc>,
    pub category: Option<String>,
    pub disk: Arc<dyn DiskSpace>,
    pub queue: TvQueueMap,
    pub user_invoked: bool,
}

impl TvSyncContext {
    fn grab(&self) -> GrabContext {
        GrabContext {
            now: self.now,
            process_start: self.process_start,
            category: self.category.clone(),
        }
    }

    fn failure_ctx(&self) -> crate::sync::SyncContext {
        crate::sync::SyncContext {
            now: self.now,
            process_start: self.process_start,
            category: None,
            disk: self.disk.clone(),
            queue: Default::default(),
            user_invoked: self.user_invoked,
        }
    }
}

#[derive(Default)]
pub struct TvRssSyncResult {
    pub fetched: usize,
    pub processed: ProcessedDecisions,
    pub failures: Vec<IndexerFailure>,
}

/// Fetches the newest TV releases from every RSS indexer and grabs what the
/// decision engine approves.
pub async fn tv_rss_sync(
    db: &Db,
    engine: &TvDecisionEngine,
    indexers: &IndexerPool,
    registry: &ClientRegistry,
    ctx: TvSyncContext,
) -> Result<(TvRssSyncResult, TvEvaluation), ServiceError> {
    let mut result = TvRssSyncResult::default();
    let mut releases = Vec::new();
    let fail_ctx = ctx.failure_ctx();

    for idx in indexers.indexers.iter().filter(|i| i.provider.enable_rss) {
        let id = idx.provider.id;
        if is_blocked(db, id, ctx.now).await? {
            continue;
        }
        let last_seen =
            db.providers()
                .last_tv_rss_sync(id)
                .await?
                .map(|(publish_date, download_url)| LastSeen {
                    publish_date,
                    download_url,
                });
        match idx.recent_tv(last_seen).await {
            Ok(feed) => {
                db.providers().record_success(id).await?;
                if let Some(newest) = &feed.newest {
                    db.providers()
                        .set_last_tv_rss_sync(id, newest.publish_date, &newest.download_url)
                        .await?;
                }
                releases.extend(feed.releases);
            }
            Err(error) => {
                tracing::warn!(indexer = %idx.provider.name, "TV RSS sync failed: {error}");
                record_failure(db, id, &error, &fail_ctx).await?;
                result.failures.push(IndexerFailure {
                    indexer: idx.provider.name.clone(),
                    error,
                });
            }
        }
    }
    result.fetched = releases.len();

    let blocked: Vec<i64> = db
        .providers()
        .blocked(ProviderKind::Indexer, ctx.now)
        .await?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    for pending in db.pending_tv_releases().list().await? {
        let r = pending.release;
        let already = releases.iter().any(|f| same_release(f, &r));
        let indexer_blocked = r.indexer_id > 0 && blocked.contains(&i64::from(r.indexer_id));
        if !already && !indexer_blocked {
            releases.push(r);
        }
    }
    let evaluation = evaluate_tv_releases(
        db,
        engine,
        TvEvaluateRequest {
            releases,
            search: None,
            pushed: false,
            now: ctx.now,
            queue: ctx.queue.clone(),
            disk: ctx.disk.clone(),
        },
    )
    .await?;
    result.processed = process_tv_decisions(db, &evaluation, registry, &ctx.grab()).await?;
    // A pending release that is now rejected for good has no reason to wait.
    if !result.processed.rejected.is_empty() {
        let pending = db.pending_tv_releases().list().await?;
        for &i in &result.processed.rejected {
            for p in pending
                .iter()
                .filter(|p| same_release(&p.release, &evaluation.items[i].release))
            {
                db.pending_tv_releases().remove(p.id).await?;
            }
        }
    }
    Ok((result, evaluation))
}

pub struct TvSearchResult {
    pub evaluation: TvEvaluation,
    pub processed: Option<ProcessedDecisions>,
    pub failures: Vec<IndexerFailure>,
}

fn tv_series(s: &SeriesRecord) -> TvSeries {
    let mut titles = vec![s.info.title.clone()];
    if !s.info.original_title.trim().is_empty() && s.info.original_title != s.info.title {
        titles.push(s.info.original_title.clone());
    }
    titles.extend(s.info.alternative_titles.iter().map(|t| t.title.clone()));
    TvSeries {
        tvdb_id: s.info.tvdb_id,
        imdb_id: s.info.imdb_id.clone(),
        tmdb_id: s.info.tmdb_id,
        series_type: s.series_type,
        titles,
    }
}

/// What to ask the indexers and how to judge the answers.
struct Query {
    search: TvSearch,
    kind: TvSearchKind,
}

fn season_query(series: &SeriesRecord, season: i32, episodes: &[EpisodeRecord]) -> Query {
    let s = tv_series(series);
    match series.series_type {
        SeriesType::Anime => Query {
            search: TvSearch::AnimeSeason {
                series: s,
                season,
                season_titles: vec![],
            },
            kind: TvSearchKind::AnimeSeason,
        },
        SeriesType::Daily => {
            let year = episodes
                .iter()
                .find_map(|e| e.air_date)
                .map_or(series.info.year, |d| d.year());
            Query {
                search: TvSearch::DailySeason { series: s, year },
                kind: TvSearchKind::Daily,
            }
        }
        SeriesType::Standard => Query {
            search: TvSearch::Season { series: s, season },
            kind: TvSearchKind::Season { season },
        },
    }
}

fn episode_query(series: &SeriesRecord, e: &EpisodeRecord) -> Query {
    let s = tv_series(series);
    if e.season_number == 0 && series.series_type != SeriesType::Daily {
        return Query {
            search: TvSearch::Special {
                series: s,
                episode_titles: vec![e.title.clone()],
            },
            kind: TvSearchKind::Special,
        };
    }
    match (series.series_type, e.air_date, e.absolute_episode_number) {
        (SeriesType::Daily, Some(air_date), _) => Query {
            search: TvSearch::DailyEpisode {
                series: s,
                air_date,
            },
            kind: TvSearchKind::Daily,
        },
        (SeriesType::Anime, _, Some(absolute)) => Query {
            search: TvSearch::AnimeEpisode {
                series: s,
                absolute,
                season: e.season_number,
                episode: e.episode_number,
            },
            kind: TvSearchKind::AnimeEpisode {
                is_season_search: false,
            },
        },
        _ => Query {
            search: TvSearch::Episode {
                series: s,
                season: e.season_number,
                episode: e.episode_number,
            },
            kind: TvSearchKind::SingleEpisode {
                season: e.season_number,
                episode: e.episode_number,
            },
        },
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_query(
    db: &Db,
    engine: &TvDecisionEngine,
    indexers: &IndexerPool,
    registry: &ClientRegistry,
    series: &SeriesRecord,
    query: Query,
    episodes: &[EpisodeRecord],
    mode: SearchMode,
    ctx: &TvSyncContext,
) -> Result<TvSearchResult, ServiceError> {
    let fail_ctx = ctx.failure_ctx();
    let mut releases = Vec::new();
    let mut failures = Vec::new();
    for idx in &indexers.indexers {
        let enabled = match mode {
            SearchMode::Automatic => idx.provider.enable_automatic_search,
            SearchMode::Interactive => idx.provider.enable_interactive_search,
        };
        let id = idx.provider.id;
        if !enabled || is_blocked(db, id, ctx.now).await? {
            continue;
        }
        match idx.search_tv(&query.search).await {
            Ok(found) => {
                db.providers().record_success(id).await?;
                releases.extend(found);
            }
            Err(error) => {
                tracing::warn!(indexer = %idx.provider.name, "TV search failed: {error}");
                record_failure(db, id, &error, &fail_ctx).await?;
                failures.push(IndexerFailure {
                    indexer: idx.provider.name.clone(),
                    error,
                });
            }
        }
    }
    let evaluation = evaluate_tv_releases(
        db,
        engine,
        TvEvaluateRequest {
            releases,
            search: Some(TvSearchContext {
                series_id: series.id,
                episode_ids: episodes.iter().map(|e| e.id).collect(),
                episodes: episodes
                    .iter()
                    .map(|e| (e.season_number, e.air_date_utc))
                    .collect(),
                kind: query.kind,
                user_invoked: ctx.user_invoked,
                interactive: mode == SearchMode::Interactive,
                monitored_only: mode == SearchMode::Automatic,
            }),
            pushed: false,
            now: ctx.now,
            queue: ctx.queue.clone(),
            disk: ctx.disk.clone(),
        },
    )
    .await?;
    let processed = match mode {
        SearchMode::Automatic => {
            let ids: Vec<i64> = episodes.iter().map(|e| e.id).collect();
            db.episodes().set_last_search_time(&ids, ctx.now).await?;
            Some(process_tv_decisions(db, &evaluation, registry, &ctx.grab()).await?)
        }
        SearchMode::Interactive => None,
    };
    Ok(TvSearchResult {
        evaluation,
        processed,
        failures,
    })
}

/// Searches for one episode.
pub async fn search_episode(
    db: &Db,
    engine: &TvDecisionEngine,
    indexers: &IndexerPool,
    registry: &ClientRegistry,
    episode_id: i64,
    mode: SearchMode,
    ctx: &TvSyncContext,
) -> Result<TvSearchResult, ServiceError> {
    let episode = db.episodes().get(episode_id).await?;
    let series = db.series().get(episode.series_id).await?;
    let query = episode_query(&series, &episode);
    run_query(
        db,
        engine,
        indexers,
        registry,
        &series,
        query,
        std::slice::from_ref(&episode),
        mode,
        ctx,
    )
    .await
}

/// Searches for a whole season with a season query.
#[allow(clippy::too_many_arguments)]
pub async fn search_season(
    db: &Db,
    engine: &TvDecisionEngine,
    indexers: &IndexerPool,
    registry: &ClientRegistry,
    series_id: i64,
    season: i32,
    mode: SearchMode,
    ctx: &TvSyncContext,
) -> Result<TvSearchResult, ServiceError> {
    let series = db.series().get(series_id).await?;
    let episodes: Vec<EpisodeRecord> = db
        .episodes()
        .for_series(series_id)
        .await?
        .into_iter()
        .filter(|e| e.season_number == season && (mode == SearchMode::Interactive || e.monitored))
        .collect();
    let query = season_query(&series, season, &episodes);
    run_query(
        db, engine, indexers, registry, &series, query, &episodes, mode, ctx,
    )
    .await
}

#[derive(Default)]
pub struct SeriesSearchSummary {
    pub searches: usize,
    pub grabbed: usize,
}

/// Looks for a series' monitored, aired episodes that have no file. A season
/// whose aired monitored episodes are all missing gets one season search;
/// anything else is searched episode by episode.
pub async fn search_missing(
    db: &Db,
    engine: &TvDecisionEngine,
    indexers: &IndexerPool,
    registry: &ClientRegistry,
    series_id: i64,
    ctx: &TvSyncContext,
) -> Result<SeriesSearchSummary, ServiceError> {
    let series = db.series().get(series_id).await?;
    let mut out = SeriesSearchSummary::default();
    if !series.monitored {
        return Ok(out);
    }
    let all = db.episodes().for_series(series_id).await?;
    let mut seasons: Vec<i32> = all.iter().map(|e| e.season_number).collect();
    seasons.sort_unstable();
    seasons.dedup();
    for season in seasons {
        let aired: Vec<&EpisodeRecord> = all
            .iter()
            .filter(|e| e.season_number == season && e.monitored && e.has_aired(ctx.now))
            .collect();
        let missing: Vec<&EpisodeRecord> =
            aired.iter().copied().filter(|e| !e.has_file()).collect();
        if missing.is_empty() {
            continue;
        }
        let results = if missing.len() == aired.len() && missing.len() > 1 && season > 0 {
            let owned: Vec<EpisodeRecord> = missing.iter().map(|e| (*e).clone()).collect();
            let q = season_query(&series, season, &owned);
            vec![
                run_query(
                    db,
                    engine,
                    indexers,
                    registry,
                    &series,
                    q,
                    &owned,
                    SearchMode::Automatic,
                    ctx,
                )
                .await?,
            ]
        } else {
            let mut v = Vec::new();
            for e in missing {
                let q = episode_query(&series, e);
                v.push(
                    run_query(
                        db,
                        engine,
                        indexers,
                        registry,
                        &series,
                        q,
                        std::slice::from_ref(e),
                        SearchMode::Automatic,
                        ctx,
                    )
                    .await?,
                );
            }
            v
        };
        for r in results {
            out.searches += 1;
            out.grabbed += r.processed.map_or(0, |p| p.grabbed.len());
        }
    }
    Ok(out)
}
