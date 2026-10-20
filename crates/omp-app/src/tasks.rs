//! The jobs that run on a schedule (and on demand through the API).

use std::{sync::Arc, time::Duration};

use omp_decision::model::QueueEntry;
use omp_parser::title::parse_movie_title;
use omp_services::{
    IndexerPool, ProcessOptions, ProcessResult, QueueMap, RssSyncResult, SearchMode, SearchResult,
    ServiceError, SyncContext, TrackedState, load_import_config, process_downloads, rss_sync,
    search_movie,
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
pub async fn process_downloads_once(app: &App) -> Result<Option<ProcessResult>, ServiceError> {
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
    Ok(Some(result))
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
                match rss_sync_once(&app).await {
                    Ok(Some(r)) => Ok(Some(format!(
                        "{} releases fetched, {} grabbed",
                        r.fetched,
                        r.processed.grabbed.len()
                    ))),
                    Ok(None) => Ok(None),
                    Err(e) => Err(e.to_string()),
                }
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

/// Looks at a movie's folder and records or forgets its file.
pub async fn rescan_movie_once(
    app: &App,
    movie_id: i64,
) -> Result<omp_services::RescanResult, ServiceError> {
    omp_services::rescan_movie(&app.db, movie_id, chrono::Utc::now()).await
}
