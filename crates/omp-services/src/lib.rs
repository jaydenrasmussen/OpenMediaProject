//! Services that tie the pure crates to storage: loading what the decision
//! engine needs, evaluating releases end to end, and (next) syncing and
//! searching.

mod config;
mod disk;
mod evaluate;
mod ffprobe;
mod formats;
mod grab;
mod import;
mod media;
mod process;
mod rename;
mod scan;
mod snapshot;
mod sync;
mod tracking;

pub use config::{DECISION_CONFIG_KEY, load_decision_config, save_decision_config};
pub use config::{load_import_config, save_import_config};
pub use disk::{DiskSpace, NoDiskCheck};
pub use evaluate::{
    EvaluateRequest, EvaluatedRelease, Evaluation, ServiceError, evaluate_releases,
};
pub use ffprobe::{Ffprobe, parse_probe_output};
pub use formats::{input_for_file, input_for_history, input_for_release, matching_ids};
pub use grab::{GrabContext, GrabbedRelease, ProcessedDecisions, grab_selected, process_decisions};
pub use import::{
    FileOutcome, FileResult, ImportConfig, ImportMode, ImportRejection, ImportRequest,
    ImportResult, Importer, MediaProbe, ProbeInfo,
};
pub use media::naming_media;
pub use process::{ProcessOptions, ProcessResult, process_downloads};
pub use rename::{RenamePreview, rename_movie, rename_preview, rename_previews};
pub use scan::{RescanResult, UnmappedFolder, rescan_movie, unmapped_folders};
pub use snapshot::{DecisionSnapshot, QueueMap};
pub use sync::{
    IndexerFailure, IndexerPool, PluginIndexers, RssSyncResult, SearchMode, SearchResult,
    SyncContext, rss_sync, search_movie, test_indexer,
};
pub use tracking::{
    DownloadMonitor, MonitorOptions, RefreshResult, TrackedDownload, TrackedState, record_failure,
};
