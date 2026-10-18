//! RSS sync and movie search: fetch from the indexers, evaluate, grab.
//! Ported from Radarr `RssSyncService` and `MovieSearchService` (GPL-3.0).

use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use omp_core::release::ReleaseInfo;
use omp_db::{
    Db,
    repo::{Provider, ProviderKind},
};
use omp_decision::{DecisionEngine, SearchContext};
use omp_downloads::ClientRegistry;
use omp_indexers::{
    Flavor, IndexerClient, IndexerError, IndexerIdentity, IndexerSettings, LastSeen, MovieSearch,
};

use crate::{
    DiskSpace, QueueMap,
    evaluate::{EvaluateRequest, Evaluation, ServiceError, evaluate_releases},
    grab::{GrabContext, ProcessedDecisions, process_decisions},
};

struct PoolIndexer {
    provider: Provider,
    client: IndexerClient,
}

/// The configured, usable indexers.
pub struct IndexerPool {
    indexers: Vec<PoolIndexer>,
}

impl IndexerPool {
    /// Builds a client for each enabled Newznab/Torznab indexer. An indexer
    /// whose settings do not validate is skipped with a warning rather than
    /// stopping the others. `rate_limit` is the gap between requests to one
    /// indexer (Radarr uses two seconds).
    pub async fn load(db: &Db, rate_limit: Duration) -> Result<Self, ServiceError> {
        let mut indexers = Vec::new();
        for provider in db.providers().list(ProviderKind::Indexer).await? {
            if !provider.enabled {
                continue;
            }
            let flavor = match provider.implementation.as_str() {
                "newznab" => Flavor::Newznab,
                "torznab" => Flavor::Torznab,
                other => {
                    tracing::debug!(indexer = %provider.name, "implementation {other} is not built in");
                    continue;
                }
            };
            let settings: IndexerSettings = match serde_json::from_value(provider.settings.clone())
            {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(indexer = %provider.name, "unreadable settings: {e}");
                    continue;
                }
            };
            let identity = IndexerIdentity {
                id: provider.id as i32,
                name: provider.name.clone(),
                priority: provider.priority,
            };
            match IndexerClient::new(flavor, settings, identity) {
                Ok(client) => indexers.push(PoolIndexer {
                    provider,
                    client: client.with_rate_limit(rate_limit),
                }),
                Err(e) => tracing::warn!(indexer = %provider.name, "invalid settings: {e}"),
            }
        }
        Ok(Self { indexers })
    }

    pub fn len(&self) -> usize {
        self.indexers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.indexers.is_empty()
    }
}

/// Everything a sync or search run needs besides the database.
pub struct SyncContext {
    pub now: DateTime<Utc>,
    pub process_start: DateTime<Utc>,
    pub category: Option<String>,
    pub disk: Arc<dyn DiskSpace>,
    pub queue: QueueMap,
    /// A person asked for this search (affects delays and some rules).
    pub user_invoked: bool,
}

impl SyncContext {
    fn grab(&self) -> GrabContext {
        GrabContext {
            now: self.now,
            process_start: self.process_start,
            category: self.category.clone(),
        }
    }
}

/// One indexer that failed during a run.
#[derive(Debug)]
pub struct IndexerFailure {
    pub indexer: String,
    pub error: IndexerError,
}

#[derive(Debug, Default)]
pub struct RssSyncResult {
    /// How many releases the indexers returned.
    pub fetched: usize,
    pub processed: ProcessedDecisions,
    pub failures: Vec<IndexerFailure>,
}

/// Records a failed call against the indexer's back-off, the way Radarr does.
async fn record_failure(
    db: &Db,
    provider_id: i64,
    error: &IndexerError,
    ctx: &SyncContext,
) -> Result<(), ServiceError> {
    let policy = error.backoff();
    db.providers()
        .record_failure(
            provider_id,
            ctx.now,
            ctx.process_start,
            policy
                .minimum
                .and_then(|d| ChronoDuration::from_std(d).ok()),
            policy.escalate,
        )
        .await?;
    Ok(())
}

async fn is_blocked(db: &Db, provider_id: i64, now: DateTime<Utc>) -> Result<bool, ServiceError> {
    Ok(db
        .providers()
        .blocked(ProviderKind::Indexer, now)
        .await?
        .iter()
        .any(|(id, _)| *id == provider_id))
}

/// Fetches the newest releases from every RSS-enabled indexer, then grabs what
/// the decision engine approves.
pub async fn rss_sync(
    db: &Db,
    engine: &DecisionEngine,
    indexers: &IndexerPool,
    registry: &ClientRegistry,
    ctx: SyncContext,
) -> Result<RssSyncResult, ServiceError> {
    let mut result = RssSyncResult::default();
    let mut releases: Vec<ReleaseInfo> = Vec::new();

    for idx in indexers.indexers.iter().filter(|i| i.provider.enable_rss) {
        let id = idx.provider.id;
        if is_blocked(db, id, ctx.now).await? {
            tracing::debug!(indexer = %idx.provider.name, "blocked by back-off, skipping");
            continue;
        }
        let last_seen =
            db.providers()
                .last_rss_sync(id)
                .await?
                .map(|(publish_date, download_url)| LastSeen {
                    publish_date,
                    download_url,
                });
        match idx.client.fetch_recent(last_seen).await {
            Ok(feed) => {
                db.providers().record_success(id).await?;
                if let Some(newest) = &feed.newest {
                    db.providers()
                        .set_last_rss_sync(id, newest.publish_date, &newest.download_url)
                        .await?;
                }
                releases.extend(feed.releases);
            }
            Err(error) => {
                tracing::warn!(indexer = %idx.provider.name, "RSS sync failed: {error}");
                record_failure(db, id, &error, &ctx).await?;
                result.failures.push(IndexerFailure {
                    indexer: idx.provider.name.clone(),
                    error,
                });
            }
        }
    }

    result.fetched = releases.len();
    // Releases waiting on a delay (or an unavailable client) are looked at
    // again every sync, along with the fresh ones.
    let blocked: Vec<i64> = db
        .providers()
        .blocked(ProviderKind::Indexer, ctx.now)
        .await?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    for pending in db.pending_releases().list().await? {
        let r = pending.release;
        let already = releases.iter().any(|f| same_release(f, &r));
        let indexer_blocked = r.indexer_id > 0 && blocked.contains(&i64::from(r.indexer_id));
        if !already && !indexer_blocked {
            releases.push(r);
        }
    }
    let evaluation = evaluate_releases(
        db,
        engine,
        EvaluateRequest {
            releases,
            search: None,
            pushed: false,
            now: ctx.now,
            queue: ctx.queue.clone(),
            disk: ctx.disk.clone(),
        },
    )
    .await?;
    result.processed = process_decisions(db, &evaluation, registry, &ctx.grab()).await?;
    remove_rejected_pending(db, &evaluation, &result.processed).await?;
    Ok(result)
}

/// The same post: same title, publish date and indexer.
fn same_release(a: &ReleaseInfo, b: &ReleaseInfo) -> bool {
    a.title == b.title && a.publish_date == b.publish_date && a.indexer == b.indexer
}

/// A release that was waiting but is now rejected for good has no reason to
/// stay in the pending list.
async fn remove_rejected_pending(
    db: &Db,
    evaluation: &Evaluation,
    processed: &ProcessedDecisions,
) -> Result<(), ServiceError> {
    if processed.rejected.is_empty() {
        return Ok(());
    }
    let pending = db.pending_releases().list().await?;
    for &i in &processed.rejected {
        for p in pending
            .iter()
            .filter(|p| same_release(&p.release, &evaluation.items[i].release))
        {
            db.pending_releases().remove(p.id).await?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// A scheduled or "search" button search: grab the best result.
    Automatic,
    /// The user wants to see the results and choose; nothing is grabbed.
    Interactive,
}

pub struct SearchResult {
    pub evaluation: Evaluation,
    /// `None` for an interactive search.
    pub processed: Option<ProcessedDecisions>,
    pub failures: Vec<IndexerFlagFailure>,
}

/// Same shape as [`IndexerFailure`]; kept as an alias for readability.
pub type IndexerFlagFailure = IndexerFailure;

/// Searches every suitable indexer for one movie.
pub async fn search_movie(
    db: &Db,
    engine: &DecisionEngine,
    indexers: &IndexerPool,
    registry: &ClientRegistry,
    movie_id: i64,
    mode: SearchMode,
    ctx: SyncContext,
) -> Result<SearchResult, ServiceError> {
    let movie = db.movies().get(movie_id).await?;
    let mut titles = vec![movie.metadata.title.clone()];
    if !movie.metadata.original_title.trim().is_empty()
        && movie.metadata.original_title != movie.metadata.title
    {
        titles.push(movie.metadata.original_title.clone());
    }
    titles.extend(
        movie
            .metadata
            .alternative_titles
            .iter()
            .map(|t| t.title.clone()),
    );
    let query = MovieSearch {
        tmdb_id: movie.movie.tmdb_id,
        imdb_id: movie.metadata.imdb_id.clone(),
        year: movie.movie.year,
        titles,
    };

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
        match idx.client.search_movie(&query).await {
            Ok(found) => {
                db.providers().record_success(id).await?;
                releases.extend(found);
            }
            Err(error) => {
                tracing::warn!(indexer = %idx.provider.name, "search failed: {error}");
                record_failure(db, id, &error, &ctx).await?;
                failures.push(IndexerFailure {
                    indexer: idx.provider.name.clone(),
                    error,
                });
            }
        }
    }

    let evaluation = evaluate_releases(
        db,
        engine,
        EvaluateRequest {
            releases,
            search: Some(SearchContext {
                movie_id,
                user_invoked: ctx.user_invoked,
                interactive: mode == SearchMode::Interactive,
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
            db.movies().set_last_search_time(movie_id, ctx.now).await?;
            Some(process_decisions(db, &evaluation, registry, &ctx.grab()).await?)
        }
        SearchMode::Interactive => None,
    };
    Ok(SearchResult {
        evaluation,
        processed,
        failures,
    })
}
