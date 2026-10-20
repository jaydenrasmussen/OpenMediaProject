//! The jobs that run on a schedule (and on demand through the API).

use std::{sync::Arc, time::Duration};

use omp_decision::{model::QueueEntry, tv::TvQueueEntry};
use omp_parser::{title::parse_movie_title, tv::parse_title as parse_tv_title};
use omp_services::{
    IndexerPool, ProcessOptions, ProcessResult, QueueMap, RssSyncResult, SearchMode, SearchResult,
    ServiceError, SyncContext, TrackedState, TvQueueMap, TvRssSyncResult, TvSearchResult,
    TvSyncContext, load_import_config, process_downloads, rss_sync, search_movie, tv_rss_sync,
};
use tokio::task::JoinHandle;

use crate::app::App;

/// What is already on its way, so the decision engine does not grab it twice.
fn queue_map(app: &App) -> QueueMap {
    let mut map = QueueMap::new();
    for t in app.monitor.snapshot() {
        if matches!(
            t.state,
            TrackedState::Imported | TrackedState::Failed | TrackedState::Ignored
        ) {
            continue;
        }
        let Some(movie_id) = t.movie_id else { continue };
        let quality = parse_movie_title(&t.item.title, false)
            .map(|p| p.quality)
            .unwrap_or_default();
        map.entry(movie_id).or_default().push(QueueEntry {
            quality,
            custom_format_ids: Vec::new(),
            failed_pending: t.state == TrackedState::FailedPending,
        });
    }
    map
}

/// As [`queue_map`], for the TV downloads on their way.
fn tv_queue_map(app: &App) -> TvQueueMap {
    let mut map = TvQueueMap::new();
    for t in app.monitor.snapshot() {
        if matches!(
            t.state,
            TrackedState::Imported | TrackedState::Failed | TrackedState::Ignored
        ) {
            continue;
        }
        let Some(series_id) = t.series_id else {
            continue;
        };
        let quality = parse_tv_title(&t.item.title)
            .map(|p| p.quality)
            .unwrap_or_default();
        map.entry(series_id).or_default().push(TvQueueEntry {
            episode_ids: t.episode_ids.clone(),
            quality,
            custom_format_ids: Vec::new(),
            failed_pending: t.state == TrackedState::FailedPending,
        });
    }
    map
}

fn tv_sync_context(app: &App, user_invoked: bool) -> TvSyncContext {
    TvSyncContext {
        now: chrono::Utc::now(),
        process_start: app.started,
        category: Some(app.config.download_category.clone()),
        disk: app.disk.clone(),
        queue: tv_queue_map(app),
        user_invoked,
    }
}

/// As [`rss_sync_once`], for TV.
pub async fn tv_rss_sync_once(app: &App) -> Result<Option<TvRssSyncResult>, ServiceError> {
    let Ok(_running) = app.tv_rss_lock.try_lock() else {
        return Ok(None);
    };
    let pool =
        IndexerPool::load_with(&app.db, Duration::from_secs(2), Some(app.plugins.clone())).await?;
    let (result, _) = tv_rss_sync(
        &app.db,
        &app.tv_engine,
        &pool,
        &app.registry(),
        tv_sync_context(app, false),
    )
    .await?;
    Ok(Some(result))
}

pub async fn search_episode_once(
    app: &App,
    episode_id: i64,
    mode: SearchMode,
    user_invoked: bool,
) -> Result<TvSearchResult, ServiceError> {
    let pool =
        IndexerPool::load_with(&app.db, Duration::from_secs(2), Some(app.plugins.clone())).await?;
    omp_services::search_episode(
        &app.db,
        &app.tv_engine,
        &pool,
        &app.registry(),
        episode_id,
        mode,
        &tv_sync_context(app, user_invoked),
    )
    .await
}

pub async fn search_season_once(
    app: &App,
    series_id: i64,
    season: i32,
    mode: SearchMode,
    user_invoked: bool,
) -> Result<TvSearchResult, ServiceError> {
    let pool =
        IndexerPool::load_with(&app.db, Duration::from_secs(2), Some(app.plugins.clone())).await?;
    omp_services::search_season(
        &app.db,
        &app.tv_engine,
        &pool,
        &app.registry(),
        series_id,
        season,
        mode,
        &tv_sync_context(app, user_invoked),
    )
    .await
}

/// Looks for the missing episodes of one series, or of all monitored ones.
/// Returns how many searches ran and how many releases were grabbed.
pub async fn search_missing_once(
    app: &App,
    only: Option<i64>,
    user_invoked: bool,
) -> Result<(usize, usize), ServiceError> {
    let pool =
        IndexerPool::load_with(&app.db, Duration::from_secs(2), Some(app.plugins.clone())).await?;
    let ids: Vec<i64> = match only {
        Some(id) => vec![id],
        None => app
            .db
            .series()
            .list()
            .await?
            .into_iter()
            .filter(|s| s.monitored)
            .map(|s| s.id)
            .collect(),
    };
    let (mut searches, mut grabbed) = (0, 0);
    for id in ids {
        let r = omp_services::search_missing(
            &app.db,
            &app.tv_engine,
            &pool,
            &app.registry(),
            id,
            &tv_sync_context(app, user_invoked),
        )
        .await?;
        searches += r.searches;
        grabbed += r.grabbed;
    }
    Ok((searches, grabbed))
}

fn sync_context(app: &App, user_invoked: bool) -> SyncContext {
    SyncContext {
        now: chrono::Utc::now(),
        process_start: app.started,
        category: Some(app.config.download_category.clone()),
        disk: app.disk.clone(),
        queue: queue_map(app),
        user_invoked,
    }
}

/// Fetches new releases from the RSS-enabled indexers and grabs what is
/// wanted. Returns `None` if a sync is already running.
pub async fn rss_sync_once(app: &App) -> Result<Option<RssSyncResult>, ServiceError> {
    let Ok(_running) = app.rss_lock.try_lock() else {
        return Ok(None);
    };
    let pool =
        IndexerPool::load_with(&app.db, Duration::from_secs(2), Some(app.plugins.clone())).await?;
    let result = rss_sync(
        &app.db,
        &app.engine,
        &pool,
        &app.registry(),
        sync_context(app, false),
    )
    .await?;
    Ok(Some(result))
}

pub async fn search_movie_once(
    app: &App,
    movie_id: i64,
    mode: SearchMode,
    user_invoked: bool,
) -> Result<SearchResult, ServiceError> {
    let pool =
        IndexerPool::load_with(&app.db, Duration::from_secs(2), Some(app.plugins.clone())).await?;
    search_movie(
        &app.db,
        &app.engine,
        &pool,
        &app.registry(),
        movie_id,
        mode,
        sync_context(app, user_invoked),
    )
    .await
}

/// Judges and, if it passes, grabs a release someone pushed to the API.
pub async fn push_release_once(
    app: &App,
    release: omp_core::release::ReleaseInfo,
) -> Result<(omp_services::Evaluation, omp_services::ProcessedDecisions), ServiceError> {
    omp_services::push_release(
        &app.db,
        &app.engine,
        &app.registry(),
        release,
        sync_context(app, false),
    )
    .await
}

/// One pass of the download loop: refresh, import, clean up.
pub async fn process_downloads_once(app: &Arc<App>) -> Result<Option<ProcessResult>, ServiceError> {
    let Ok(_running) = app.process_lock.try_lock() else {
        return Ok(None);
    };
    let config = load_import_config(&app.db).await?;
    let result = process_downloads(
        &app.db,
        &app.monitor,
        &app.registry(),
        &config,
        app.probe.as_ref(),
        app.disk.as_ref(),
        &ProcessOptions {
            now: chrono::Utc::now(),
            category: Some(app.config.download_category.clone()),
            remove_failed: true,
            remove_completed: true,
        },
    )
    .await?;
    if config.redownload_failed {
        for id in &result.refresh.newly_failed {
            if let Some(tracked) = app.monitor.get(id) {
                redownload(app, &tracked);
            }
        }
    }
    Ok(Some(result))
}

/// Looks for another release after a download failed (its release is
/// blocklisted by then). Runs in the background; a failed search is logged.
fn redownload(app: &Arc<App>, tracked: &omp_services::TrackedDownload) {
    let movie_id = tracked.movie_id;
    let series_id = tracked.series_id;
    let episode_ids = tracked.episode_ids.clone();
    let app = app.clone();
    tokio::spawn(async move {
        let result = if let Some(id) = movie_id {
            search_movie_once(&app, id, SearchMode::Automatic, false)
                .await
                .map(|_| ())
        } else if let (Some(_), [episode]) = (series_id, episode_ids.as_slice()) {
            search_episode_once(&app, *episode, SearchMode::Automatic, false)
                .await
                .map(|_| ())
        } else if let Some(series) = series_id {
            search_missing_once(&app, Some(series), false)
                .await
                .map(|_| ())
        } else {
            Ok(())
        };
        if let Err(e) = result {
            tracing::warn!("search after a failed download failed: {e}");
        }
    });
}

/// Runs `job` every `every` (after `delay`), recording each run for the
/// Tasks page. `job` returns `Ok(Some(note))` to log what it did.
fn every<F, Fut>(
    app: &Arc<App>,
    name: &'static str,
    label: &'static str,
    delay: Duration,
    every: Duration,
    skip_first: bool,
    job: F,
) -> JoinHandle<()>
where
    F: Fn(Arc<App>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<Option<String>, String>> + Send,
{
    app.register_task(name, label, every);
    let app = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let mut tick = tokio::time::interval(every);
        if skip_first {
            tick.tick().await;
        }
        loop {
            tick.tick().await;
            let started = chrono::Utc::now();
            let result = job(app.clone()).await;
            match &result {
                Ok(Some(note)) => tracing::info!("{label}: {note}"),
                Ok(None) => {}
                Err(e) => tracing::warn!("{label} failed: {e}"),
            }
            app.finish_task(name, started, result.err());
        }
    })
}

/// Starts the periodic jobs. Abort the handles to stop them.
pub fn spawn_scheduler(app: Arc<App>) -> Vec<JoinHandle<()>> {
    let minute = Duration::from_secs(60);
    vec![
        every(
            &app,
            "rss-sync",
            "RSS sync",
            Duration::from_secs(30),
            app.config.rss_interval,
            false,
            |app| async move {
                let movies = rss_sync_once(&app).await.map_err(|e| e.to_string())?;
                let tv = tv_rss_sync_once(&app).await.map_err(|e| e.to_string())?;
                Ok(match (movies, tv) {
                    (None, None) => None,
                    (m, t) => Some(format!(
                        "{} releases fetched, {} grabbed",
                        m.as_ref().map_or(0, |r| r.fetched) + t.as_ref().map_or(0, |r| r.fetched),
                        m.as_ref().map_or(0, |r| r.processed.grabbed.len())
                            + t.as_ref().map_or(0, |r| r.processed.grabbed.len())
                    )),
                })
            },
        ),
        every(
            &app,
            "process-downloads",
            "Check downloads",
            Duration::ZERO,
            app.config.process_interval,
            false,
            |app| async move {
                match process_downloads_once(&app).await {
                    Ok(Some(r)) if !r.imported.is_empty() || !r.refresh.newly_failed.is_empty() => {
                        Ok(Some(format!(
                            "{} imported, {} failed",
                            r.imported.len(),
                            r.refresh.newly_failed.len()
                        )))
                    }
                    Ok(_) => Ok(None),
                    Err(e) => Err(e.to_string()),
                }
            },
        ),
        every(
            &app,
            "backup",
            "Backup",
            Duration::ZERO,
            app.config.backup_interval,
            true,
            |app| async move {
                crate::backups::create(&app)
                    .await
                    .map(|b| Some(format!("{} ({} bytes)", b.name, b.size)))
                    .map_err(|e| e.to_string())
            },
        ),
        every(
            &app,
            "refresh-movies",
            "Refresh movie details",
            5 * minute,
            app.config.refresh_interval,
            false,
            |app| async move {
                refresh_movies_once(&app, None)
                    .await
                    .map(|n| Some(format!("{n} movies refreshed")))
                    .map_err(|e| e.to_string())
            },
        ),
        every(
            &app,
            "refresh-series",
            "Refresh series details",
            6 * minute,
            app.config.refresh_interval,
            false,
            |app| async move {
                crate::tv_api::refresh(&app, None)
                    .await
                    .map(|n| Some(format!("{n} series refreshed")))
            },
        ),
        every(
            &app,
            "recycle-bin",
            "Clean the recycle bin",
            10 * minute,
            Duration::from_secs(24 * 3600),
            false,
            |app| async move {
                let config = load_import_config(&app.db)
                    .await
                    .map_err(|e| e.to_string())?;
                omp_services::clean_recycle_bin(&config)
                    .await
                    .map(|n| (n > 0).then(|| format!("{n} items removed")))
                    .map_err(|e| e.to_string())
            },
        ),
        every(
            &app,
            "notifications",
            "Send notifications",
            Duration::ZERO,
            Duration::from_secs(15),
            false,
            |app| async move {
                crate::notify::dispatch_once(&app)
                    .await
                    .map(|n| (n > 0).then(|| format!("{n} sent")))
                    .map_err(|e| e.to_string())
            },
        ),
        every(
            &app,
            "import-lists",
            "Sync import lists",
            2 * minute,
            app.config.list_interval,
            false,
            |app| async move {
                match crate::lists::sync_once(&app).await {
                    Ok(r) if r.problems.is_empty() => {
                        Ok((r.added > 0).then(|| format!("{} added", r.added)))
                    }
                    Ok(r) => Err(r.problems.join("; ")),
                    Err(e) => Err(e.to_string()),
                }
            },
        ),
    ]
}

/// Re-reads movie details from the metadata provider: release dates, status
/// and artwork change after a movie is added. Returns how many were updated.
/// One movie failing (renamed or removed upstream) does not stop the rest.
pub async fn refresh_movies_once(app: &App, only: Option<i64>) -> Result<usize, ServiceError> {
    let Some(provider) = app.metadata.clone() else {
        return Ok(0);
    };
    let movies = match only {
        Some(id) => vec![app.db.movies().get(id).await?],
        None => app.db.movies().list().await?,
    };
    let mut updated = 0;
    for m in &movies {
        match provider.movie(m.movie.tmdb_id).await {
            Ok(meta) => {
                app.db.movies().upsert_metadata(&meta).await?;
                updated += 1;
            }
            Err(e) => tracing::warn!(movie = %m.movie.title, "metadata refresh failed: {e}"),
        }
        if only.is_none() {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    Ok(updated)
}

/// Renames files to match the naming format: one movie, or the whole
/// library. A file that cannot be renamed is logged and skipped. Returns how
/// many were renamed.
pub async fn rename_movies_once(app: &App, only: Option<i64>) -> Result<usize, ServiceError> {
    let naming = load_import_config(&app.db).await?.naming;
    let ids: Vec<i64> = match only {
        Some(id) => vec![id],
        None => omp_services::rename_previews(&app.db, &naming)
            .await?
            .into_iter()
            .map(|p| p.movie_id)
            .collect(),
    };
    let mut renamed = 0;
    for id in ids {
        match omp_services::rename_movie(&app.db, &naming, id, chrono::Utc::now()).await {
            Ok(Some(_)) => renamed += 1,
            Ok(None) => {}
            Err(e) if only.is_some() => return Err(e),
            Err(e) => tracing::warn!(movie_id = id, "rename failed: {e}"),
        }
    }
    Ok(renamed)
}

/// Renames series files to match the naming format: one series or all.
pub async fn rename_series_once(app: &App, only: Option<i64>) -> Result<usize, ServiceError> {
    let naming = load_import_config(&app.db).await?.naming;
    let ids: Vec<i64> = match only {
        Some(id) => vec![id],
        None => omp_services::episode_rename_previews(&app.db, &naming, None)
            .await?
            .into_iter()
            .map(|p| p.series_id)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect(),
    };
    let mut renamed = 0;
    for id in ids {
        match omp_services::rename_series(&app.db, &naming, id, chrono::Utc::now()).await {
            Ok(n) => renamed += n,
            Err(e) if only.is_some() => return Err(e),
            Err(e) => tracing::warn!(series_id = id, "rename failed: {e}"),
        }
    }
    Ok(renamed)
}

/// Looks at a movie's folder and records or forgets its file.
pub async fn rescan_movie_once(
    app: &App,
    movie_id: i64,
) -> Result<omp_services::RescanResult, ServiceError> {
    omp_services::rescan_movie(&app.db, movie_id, chrono::Utc::now()).await
}
