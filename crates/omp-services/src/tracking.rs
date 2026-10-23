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
use omp_db::{
    Db,
    repo::{EpisodeHistoryRecord, HistoryRecord, RemotePathMapping, TvBlocklistEntry},
};
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
    /// For a TV grab: the series and the episodes it covers.
    pub series_id: Option<i64>,
    pub episode_ids: Vec<i64>,
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

fn state_from_history(
    history: &[HistoryRecord],
    episode_history: &[EpisodeHistoryRecord],
) -> TrackedState {
    let events = history
        .iter()
        .map(|h| h.event)
        .chain(episode_history.iter().map(|h| h.event));
    events
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .find_map(|e| match e {
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

    /// Stops tracking a download (it was removed on purpose).
    pub fn forget(&self, download_id: &str) {
        self.tracked.lock().unwrap().remove(download_id);
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
        let mappings = db.remote_path_mappings().list().await?;

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
            for mut item in items {
                if let Some(path) = item.output_path.take() {
                    item.output_path = Some(remap_path(&mappings, rc.id, &path));
                }
                let history = db.history().by_download_id(&item.download_id).await?;
                let grabbed = history.iter().rfind(|h| h.event == HistoryEvent::Grabbed);
                let ep_history = db
                    .episode_history()
                    .by_download_id(&item.download_id)
                    .await?;
                let tv_grabs: Vec<&EpisodeHistoryRecord> = ep_history
                    .iter()
                    .filter(|h| h.event == HistoryEvent::Grabbed)
                    .collect();
                let ours = grabbed.is_some()
                    || !tv_grabs.is_empty()
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
                    state: state_from_history(&history, &ep_history),
                    movie_id: grabbed.map(|g| g.movie_id),
                    series_id: tv_grabs.first().map(|g| g.series_id),
                    episode_ids: tv_grabs.iter().map(|g| g.episode_id).collect(),
                    indexer: grabbed.and_then(|g| g.data.get("indexer").cloned()),
                    added: grabbed
                        .map(|g| g.date)
                        .or_else(|| tv_grabs.first().map(|g| g.date)),
                    warnings: Vec::new(),
                    trackable: true,
                });
                entry.item = item;
                entry.trackable = true;
                if entry.movie_id.is_none() {
                    entry.movie_id = grabbed.map(|g| g.movie_id);
                }
                if entry.series_id.is_none() && !tv_grabs.is_empty() {
                    entry.series_id = tv_grabs.first().map(|g| g.series_id);
                    entry.episode_ids = tv_grabs.iter().map(|g| g.episode_id).collect();
                }

                if entry.state == TrackedState::Downloading
                    && entry.item.status == DownloadItemStatus::Failed
                    && mark_failed(db, entry, grabbed, &tv_grabs, options).await?
                {
                    result.newly_failed.push(id.clone());
                    if options.remove_failed {
                        let _ = rc.client.remove(&id, true).await;
                    }
                }
                let was_waiting = matches!(entry.state, TrackedState::ImportPending);
                check_completed(entry, grabbed.is_some() || !tv_grabs.is_empty());
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

/// Turns a path as the download client sees it into one we can read, using
/// the longest matching mapping for that client (or for every client). A path
/// no mapping covers is returned as it was.
pub fn remap_path(mappings: &[RemotePathMapping], client_id: i64, path: &str) -> String {
    let separator = |p: &str| {
        if p.contains('\\') && !p.contains('/') {
            '\\'
        } else {
            '/'
        }
    };
    let mut best: Option<(&RemotePathMapping, usize)> = None;
    for m in mappings
        .iter()
        .filter(|m| m.client_id.is_none_or(|c| c == client_id))
    {
        let remote = m.remote_path.trim_end_matches(['/', '\\']);
        if remote.is_empty() {
            continue;
        }
        let matches = path.len() >= remote.len()
            && path.is_char_boundary(remote.len())
            && path[..remote.len()].eq_ignore_ascii_case(remote)
            && (path.len() == remote.len()
                || matches!(path.as_bytes()[remote.len()], b'/' | b'\\'));
        if matches && best.is_none_or(|(_, len)| remote.len() > len) {
            best = Some((m, remote.len()));
        }
    }
    let Some((m, len)) = best else {
        return path.to_owned();
    };
    let rest = &path[len..];
    let sep = separator(&m.remote_path);
    let rest = if sep == '\\' {
        rest.replace('\\', "/")
    } else {
        rest.to_owned()
    };
    format!("{}{}", m.local_path.trim_end_matches(['/', '\\']), rest)
}

/// Failed in the client: record it, blocklist the release so it is not picked
/// again, and mark it failed. Returns whether it was recorded.
async fn mark_failed(
    db: &Db,
    t: &mut TrackedDownload,
    grabbed: Option<&HistoryRecord>,
    tv_grabs: &[&EpisodeHistoryRecord],
    options: &MonitorOptions,
) -> Result<bool, ServiceError> {
    if grabbed.is_none() && !tv_grabs.is_empty() {
        t.state = TrackedState::FailedPending;
        let message = t
            .item
            .message
            .clone()
            .unwrap_or_else(|| "Download failed".into());
        record_tv_failure(
            db,
            tv_grabs,
            &t.item.download_id,
            t.protocol,
            message,
            options.now,
        )
        .await?;
        t.state = TrackedState::Failed;
        return Ok(true);
    }
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
    record_failure(
        db,
        grab,
        &t.item.download_id,
        t.protocol,
        message,
        options.now,
    )
    .await?;
    t.state = TrackedState::Failed;
    Ok(true)
}

/// Writes the "download failed" history event for a grab and blocklists the
/// release so it is not picked again.
pub async fn record_failure(
    db: &Db,
    grab: &HistoryRecord,
    download_id: &str,
    protocol: DownloadProtocol,
    message: String,
    now: DateTime<Utc>,
) -> Result<(), ServiceError> {
    db.history()
        .insert(&HistoryRecord {
            id: 0,
            movie_id: grab.movie_id,
            event: HistoryEvent::DownloadFailed,
            date: now,
            source_title: grab.source_title.clone(),
            quality: grab.quality,
            languages: grab.languages.clone(),
            download_id: Some(download_id.to_owned()),
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
            protocol,
            indexer: data.get("indexer").cloned(),
            indexer_flags: IndexerFlags(
                data.get("indexer_flags")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
            ),
            torrent_info_hash: data.get("torrent_info_hash").cloned(),
            message: Some(message),
            date: now,
        })
        .await?;
    Ok(())
}

/// As [`record_failure`] for a TV grab: one failed event per episode and one
/// blocklist entry for the release.
pub async fn record_tv_failure(
    db: &Db,
    grabs: &[&EpisodeHistoryRecord],
    download_id: &str,
    protocol: DownloadProtocol,
    message: String,
    now: DateTime<Utc>,
) -> Result<(), ServiceError> {
    let Some(first) = grabs.first() else {
        return Ok(());
    };
    for g in grabs {
        db.episode_history()
            .insert(&EpisodeHistoryRecord {
                id: 0,
                series_id: g.series_id,
                episode_id: g.episode_id,
                event: HistoryEvent::DownloadFailed,
                date: now,
                source_title: g.source_title.clone(),
                quality: g.quality,
                languages: g.languages.clone(),
                download_id: Some(download_id.to_owned()),
                data: [
                    ("message".to_owned(), message.clone()),
                    (
                        "indexer".to_owned(),
                        g.data.get("indexer").cloned().unwrap_or_default(),
                    ),
                ]
                .into(),
            })
            .await?;
    }
    let data = &first.data;
    db.tv_blocklist()
        .insert(&TvBlocklistEntry {
            entry: BlocklistEntry {
                id: 0,
                movie_id: first.series_id,
                source_title: first.source_title.clone(),
                quality: first.quality,
                languages: first.languages.clone(),
                published_date: data
                    .get("published_date")
                    .and_then(|d| DateTime::parse_from_rfc3339(d).ok())
                    .map(|d| d.with_timezone(&Utc)),
                size: data.get("size").and_then(|s| s.parse().ok()),
                protocol,
                indexer: data.get("indexer").cloned(),
                indexer_flags: IndexerFlags(
                    data.get("indexer_flags")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0),
                ),
                torrent_info_hash: data.get("torrent_info_hash").cloned(),
                message: Some(message),
                date: now,
            },
            episode_ids: grabs.iter().map(|g| g.episode_id).collect(),
        })
        .await?;
    Ok(())
}

/// A finished download that is still waiting moves to ImportPending when it
/// can be tied to a movie, and to ImportBlocked when it cannot.
fn check_completed(t: &mut TrackedDownload, grabbed: bool) {
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
    if !grabbed && t.movie_id.is_none() && t.series_id.is_none() {
        t.warn("Title mismatch, automatic import is not possible. Manual import required.");
        t.state = TrackedState::ImportBlocked;
        return;
    }
    t.warnings.clear();
    t.state = TrackedState::ImportPending;
}

#[cfg(test)]
mod remap_tests {
    use super::*;

    fn m(id: i64, client: Option<i64>, remote: &str, local: &str) -> RemotePathMapping {
        RemotePathMapping {
            id,
            client_id: client,
            remote_path: remote.into(),
            local_path: local.into(),
        }
    }

    #[test]
    fn paths_are_translated_by_the_longest_matching_mapping() {
        let maps = [
            m(1, None, "/downloads/", "/mnt/dl"),
            m(2, Some(7), "/downloads/tv", "/mnt/tv-dl/"),
            m(3, Some(8), "D:\\Torrents", "/mnt/torrents"),
        ];
        assert_eq!(
            remap_path(&maps, 1, "/downloads/movies/x.mkv"),
            "/mnt/dl/movies/x.mkv"
        );
        assert_eq!(
            remap_path(&maps, 7, "/downloads/tv/Show/e.mkv"),
            "/mnt/tv-dl/Show/e.mkv"
        );
        assert_eq!(
            remap_path(&maps, 1, "/downloads/tv/Show"),
            "/mnt/dl/tv/Show",
            "mapping 2 is for client 7"
        );
        assert_eq!(
            remap_path(&maps, 8, "D:\\Torrents\\Show\\e.mkv"),
            "/mnt/torrents/Show/e.mkv"
        );
        assert_eq!(
            remap_path(&maps, 8, "d:\\torrents"),
            "/mnt/torrents",
            "drive letters ignore case"
        );
        assert_eq!(
            remap_path(&maps, 1, "/downloads2/x"),
            "/downloads2/x",
            "only whole path segments match"
        );
        assert_eq!(remap_path(&maps, 1, "/elsewhere/x"), "/elsewhere/x");
    }
}
