//! Glue: poll the download clients, then import what has finished.

use std::path::Path;

use chrono::{DateTime, Utc};
use omp_db::Db;
use omp_downloads::ClientRegistry;

use crate::{
    DiskSpace,
    evaluate::ServiceError,
    import::{FileOutcome, ImportConfig, ImportRequest, Importer, MediaProbe},
    tracking::{DownloadMonitor, MonitorOptions, RefreshResult, TrackedState},
    tv_import::{TvImportRequest, TvImporter},
};

pub struct ProcessOptions {
    pub now: DateTime<Utc>,
    pub category: Option<String>,
    pub remove_failed: bool,
    /// Take a finished, imported download out of its client.
    pub remove_completed: bool,
}

#[derive(Debug, Default)]
pub struct ProcessResult {
    pub refresh: RefreshResult,
    pub imported: Vec<String>,
    /// Downloads that need a person, with the reasons.
    pub blocked: Vec<(String, Vec<String>)>,
}

/// One pass of the download loop: refresh the tracked downloads, then import
/// each one that is ready.
pub async fn process_downloads(
    db: &Db,
    monitor: &DownloadMonitor,
    registry: &ClientRegistry,
    config: &ImportConfig,
    probe: &dyn MediaProbe,
    disk: &dyn DiskSpace,
    options: &ProcessOptions,
) -> Result<ProcessResult, ServiceError> {
    let refresh = monitor
        .refresh(
            db,
            registry,
            &MonitorOptions {
                now: options.now,
                category: options.category.clone(),
                remove_failed: options.remove_failed,
            },
        )
        .await?;
    let mut result = ProcessResult {
        refresh,
        ..Default::default()
    };

    let importer = Importer {
        db,
        config,
        probe,
        disk,
    };
    let tv_importer = TvImporter {
        db,
        config,
        probe,
        disk,
    };
    for tracked in monitor.snapshot() {
        if tracked.state != TrackedState::ImportPending {
            continue;
        }
        let id = tracked.item.download_id.clone();
        let Some(output) = tracked.item.output_path.as_deref() else {
            continue;
        };
        let imported = if let Some(series_id) = tracked.series_id {
            monitor.set_state(&id, TrackedState::Importing);
            tv_importer
                .import(&TvImportRequest {
                    series_id,
                    output_path: Path::new(output),
                    download_id: Some(&id),
                    download_title: &tracked.item.title,
                    episode_ids: &tracked.episode_ids,
                    can_move_files: tracked.item.can_move_files,
                    now: options.now,
                })
                .await?
        } else if let Some(movie_id) = tracked.movie_id {
            monitor.set_state(&id, TrackedState::Importing);
            importer
                .import(&ImportRequest {
                    movie_id,
                    output_path: Path::new(output),
                    download_id: Some(&id),
                    download_title: &tracked.item.title,
                    can_move_files: tracked.item.can_move_files,
                    now: options.now,
                })
                .await?
        } else {
            continue;
        };

        if imported.imported() {
            monitor.set_state(&id, TrackedState::Imported);
            result.imported.push(id.clone());
            if options.remove_completed
                && tracked.item.can_be_removed
                && let Some(client) = registry.get(tracked.client_id)
                && let Err(e) = client.client.remove(&id, false).await
            {
                tracing::warn!(download = %id, "could not remove from client: {e}");
            }
            continue;
        }

        let mut reasons: Vec<String> = Vec::new();
        for f in &imported.files {
            let name = f
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match &f.outcome {
                FileOutcome::Rejected(rs) => {
                    reasons.extend(rs.iter().map(|r| format!("{name}: {r}")));
                }
                FileOutcome::Failed(m) => reasons.push(format!("{name}: {m}")),
                FileOutcome::Imported { .. } => {}
            }
        }
        if reasons.is_empty() {
            reasons.push(format!(
                "No files found are eligible for import in {output}"
            ));
        }
        monitor.block(&id, reasons.clone());
        result.blocked.push((id, reasons));
    }
    Ok(result)
}
