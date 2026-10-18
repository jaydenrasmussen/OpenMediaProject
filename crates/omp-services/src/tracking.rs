//! Following grabbed downloads through the download clients: which movie each
//! belongs to, whether it finished or failed, and when it is ready to import.
//! Ported from Radarr `TrackedDownloadService`, `DownloadMonitoringService`,
//! `CompletedDownloadService.Check` and `FailedDownloadService` (GPL-3.0).
//!
//! A download is tied to its movie through the grab history row that shares
//! its download id. Downloads that were not grabbed here are only picked up
//! if they sit in the configured category, and then need manual import.

use std::{collections::HashMap, sync::Mutex};

use chrono::{DateTime, Utc};
use omp_core::{
    blocklist::BlocklistEntry,
    history::HistoryEvent,
    release::{DownloadProtocol, IndexerFlags},
};
use omp_db::{Db, repo::HistoryRecord};
use omp_downloads::{ClientItem, ClientRegistry, DownloadItemStatus};

use crate::evaluate::ServiceError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackedState {
    Downloading,
    /// Finished, but something needs a person (movie unknown, import failed).
    ImportBlocked,
    ImportPending,
    Importing,
    Imported,
    FailedPending,
    Failed,
    Ignored,
}

#[derive(Debug, Clone)]
pub struct TrackedDownload {
    pub client_id: i64,
    pub client_name: String,
    pub protocol: DownloadProtocol,
    pub item: ClientItem,
    pub state: TrackedState,
    pub movie_id: Option<i64>,
    pub indexer: Option<String>,
    pub added: Option<DateTime<Utc>>,
    pub warnings: Vec<String>,
    /// Still reported by its client at the last refresh.
    pub trackable: bool,
}

impl TrackedDownload {
    fn warn(&mut self, message: impl Into<String>) {
        let message = message.into();
        if !self.warnings.contains(&message) {
            self.warnings.push(message);
        }
    }
}

#[derive(Debug, Default)]
pub struct RefreshResult {
    /// Downloads that became ready to import during this refresh.
    pub newly_pending: Vec<String>,
    /// Downloads recorded as failed (and blocklisted) during this refresh.
    pub newly_failed: Vec<String>,
    /// Clients that could not be reached.
    pub unreachable: Vec<String>,
}

pub struct MonitorOptions {
    pub now: DateTime<Utc>,
    /// The category (label) whose downloads are ours even if we did not grab them.
    pub category: Option<String>,
    /// Remove failed downloads from the client.
    pub remove_failed: bool,
}

#[derive(Default)]
pub struct DownloadMonitor {
    tracked: Mutex<HashMap<String, TrackedDownload>>,
}

fn state_from_history(history: &[HistoryRecord]) -> TrackedState {
    history
        .iter()
        .rev()
        .find_map(|h| match h.event {
            HistoryEvent::DownloadFolderImported => Some(TrackedState::Imported),
            HistoryEvent::DownloadFailed => Some(TrackedState::Failed),
            HistoryEvent::DownloadIgnored => Some(TrackedState::Ignored),
            _ => None,
        })
        .unwrap_or(TrackedState::Downloading)
}

impl DownloadMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, download_id: &str) -> Option<TrackedDownload> {
        self.tracked.lock().unwrap().get(download_id).cloned()
    }

    /// Everything currently tracked, in a stable order.
    pub fn snapshot(&self) -> Vec<TrackedDownload> {
        let mut all: Vec<_> = self.tracked.lock().unwrap().values().cloned().collect();
        all.sort_by(|a, b| a.item.download_id.cmp(&b.item.download_id));
        all
    }

    /// Marks a download as needing a person, with the reasons.
    pub fn block(&self, download_id: &str, reasons: Vec<String>) {
        if let Some(t) = self.tracked.lock().unwrap().get_mut(download_id) {
            t.state = TrackedState::ImportBlocked;
            t.warnings = reasons;
        }
    }

    /// Records the outcome of an import attempt made elsewhere.
    pub fn set_state(&self, download_id: &str, state: TrackedState) {
        if let Some(t) = self.tracked.lock().unwrap().get_mut(download_id) {
            t.state = state;
        }
    }

    /// Polls every client and updates what is tracked. Call from one task at
    /// a time.
    pub async fn refresh(
        &self,
        db: &Db,
        registry: &ClientRegistry,
        options: &MonitorOptions,
    ) -> Result<RefreshResult, ServiceError> {
        let mut map = std::mem::take(&mut *self.tracked.lock().unwrap());
        let result = self.refresh_into(&mut map, db, registry, options).await;
        *self.tracked.lock().unwrap() = map;
        result
    }

    async fn refresh_into(
        &self,
        map: &mut HashMap<String, TrackedDownload>,
        db: &Db,
        registry: &ClientRegistry,
        options: &MonitorOptions,
    ) -> Result<RefreshResult, ServiceError> {
        let mut result = RefreshResult::default();
        for t in map.values_mut() {
            t.trackable = false;
        }
        let mut reached = Vec::new();

        for rc in registry.all() {
            let items = match rc.client.items().await {
                Ok(items) => items,
                Err(e) => {
                    tracing::warn!(client = %rc.name, "could not list downloads: {e}");
                    result.unreachable.push(rc.name.clone());
                    // Keep what we knew about this client's downloads.
                    for t in map.values_mut().filter(|t| t.client_id == rc.id) {
                        t.trackable = true;
                    }
                    continue;
                }
            };
            reached.push(rc.id);
            for item in items {
                let history = db.history().by_download_id(&item.download_id).await?;
                let grabbed = history.iter().rfind(|h| h.event == HistoryEvent::Grabbed);
                let ours = grabbed.is_some()
                    || options.category.is_some() && item.category == options.category;
                if !ours {
                    continue;
                }

                let id = item.download_id.clone();
                let entry = map.entry(id.clone()).or_insert_with(|| TrackedDownload {
                    client_id: rc.id,
                    client_name: rc.name.clone(),
                    protocol: rc.protocol(),
                    item: item.clone(),
                    state: state_from_history(&history),
                    movie_id: grabbed.map(|g| g.movie_id),
                    indexer: grabbed.and_then(|g| g.data.get("indexer").cloned()),
                    added: grabbed.map(|g| g.date),
                    warnings: Vec::new(),
                    trackable: true,
                });
                entry.item = item;
                entry.trackable = true;
                if entry.movie_id.is_none() {
                    entry.movie_id = grabbed.map(|g| g.movie_id);
                }

                if entry.state == TrackedState::Downloading
                    && entry.item.status == DownloadItemStatus::Failed
                    && mark_failed(db, entry, grabbed, options).await?
                {
                    result.newly_failed.push(id.clone());
                    if options.remove_failed {
                        let _ = rc.client.remove(&id, true).await;
                    }
                }
                let was_waiting = matches!(entry.state, TrackedState::ImportPending);
                check_completed(entry, grabbed);
                if !was_waiting && entry.state == TrackedState::ImportPending {
                    result.newly_pending.push(id);
                }
            }
        }

        // Forget downloads whose client no longer reports them.
        map.retain(|_, t| t.trackable || !reached.contains(&t.client_id));
        Ok(result)
    }
}

/// Failed in the client: record it, blocklist the release so it is not picked
/// again, and mark it failed. Returns whether it was recorded.
async fn mark_failed(
    db: &Db,
    t: &mut TrackedDownload,
    grabbed: Option<&HistoryRecord>,
    options: &MonitorOptions,
) -> Result<bool, ServiceError> {
    let Some(grab) = grabbed else {
        t.warn("Download wasn't grabbed here, skipping failure handling.");
        return Ok(false);
    };
    t.state = TrackedState::FailedPending;
    let message = t
        .item
        .message
        .clone()
        .unwrap_or_else(|| "Download failed".into());

    db.history()
        .insert(&HistoryRecord {
            id: 0,
            movie_id: grab.movie_id,
            event: HistoryEvent::DownloadFailed,
            date: options.now,
            source_title: grab.source_title.clone(),
            quality: grab.quality,
            languages: grab.languages.clone(),
            download_id: Some(t.item.download_id.clone()),
            data: [
                ("message".to_owned(), message.clone()),
                (
                    "indexer".to_owned(),
                    grab.data.get("indexer").cloned().unwrap_or_default(),
                ),
            ]
            .into(),
        })
        .await?;

    let data = &grab.data;
    db.blocklist()
        .insert(&BlocklistEntry {
            id: 0,
            movie_id: grab.movie_id,
            source_title: grab.source_title.clone(),
            quality: grab.quality,
            languages: grab.languages.clone(),
            published_date: data
                .get("published_date")
                .and_then(|d| DateTime::parse_from_rfc3339(d).ok())
                .map(|d| d.with_timezone(&Utc)),
            size: data.get("size").and_then(|s| s.parse().ok()),
            protocol: t.protocol,
            indexer: data.get("indexer").cloned(),
            indexer_flags: IndexerFlags(
                data.get("indexer_flags")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
            ),
            torrent_info_hash: data.get("torrent_info_hash").cloned(),
            message: Some(message),
            date: options.now,
        })
        .await?;
    t.state = TrackedState::Failed;
    Ok(true)
}

/// A finished download that is still waiting moves to ImportPending when it
/// can be tied to a movie, and to ImportBlocked when it cannot.
fn check_completed(t: &mut TrackedDownload, grabbed: Option<&HistoryRecord>) {
    if t.item.status != DownloadItemStatus::Completed {
        return;
    }
    if !matches!(
        t.state,
        TrackedState::Downloading | TrackedState::ImportBlocked
    ) {
        return;
    }
    if t.item.output_path.as_deref().is_none_or(str::is_empty) {
        t.warn("Download client did not report where the files are.");
        return;
    }
    if grabbed.is_none() && t.movie_id.is_none() {
        t.warn("Movie title mismatch, automatic import is not possible. Manual import required.");
        t.state = TrackedState::ImportBlocked;
        return;
    }
    t.warnings.clear();
    t.state = TrackedState::ImportPending;
}
