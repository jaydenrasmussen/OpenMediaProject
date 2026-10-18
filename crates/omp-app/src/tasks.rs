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
    let pool = IndexerPool::load(&app.db, Duration::from_secs(2)).await?;
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
    let pool = IndexerPool::load(&app.db, Duration::from_secs(2)).await?;
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

/// Starts the periodic jobs. Abort the handles to stop them.
pub fn spawn_scheduler(app: Arc<App>) -> Vec<JoinHandle<()>> {
    let rss = {
        let app = app.clone();
        tokio::spawn(async move {
            // Give the download clients a moment before the first sync.
            tokio::time::sleep(Duration::from_secs(30)).await;
            let mut tick = tokio::time::interval(app.config.rss_interval);
            loop {
                tick.tick().await;
                match rss_sync_once(&app).await {
                    Ok(Some(r)) => tracing::info!(
                        fetched = r.fetched,
                        grabbed = r.processed.grabbed.len(),
                        "RSS sync done"
                    ),
                    Ok(None) => tracing::debug!("RSS sync already running"),
                    Err(e) => tracing::warn!("RSS sync failed: {e}"),
                }
            }
        })
    };
    let process = {
        let app = app.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(app.config.process_interval);
            loop {
                tick.tick().await;
                match process_downloads_once(&app).await {
                    Ok(Some(r)) if !r.imported.is_empty() || !r.refresh.newly_failed.is_empty() => {
                        tracing::info!(
                            imported = r.imported.len(),
                            failed = r.refresh.newly_failed.len(),
                            "downloads processed"
                        );
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!("download processing failed: {e}"),
                }
            }
        })
    };
    let backup = {
        let app = app.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(app.config.backup_interval);
            tick.tick().await; // the first tick is immediate; a backup at every start would be noise
            loop {
                tick.tick().await;
                match crate::backups::create(&app).await {
                    Ok(b) => tracing::info!(name = %b.name, size = b.size, "database backed up"),
                    Err(e) => tracing::warn!("backup failed: {e}"),
                }
            }
        })
    };
    vec![rss, process, backup]
}
