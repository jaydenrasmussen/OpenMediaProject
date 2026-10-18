//! Services that tie the pure crates to storage: loading what the decision
//! engine needs, evaluating releases end to end, and (next) syncing and
//! searching.

mod config;
mod disk;
mod evaluate;
mod formats;
mod grab;
mod import;
mod snapshot;
mod sync;
mod tracking;

pub use config::{DECISION_CONFIG_KEY, load_decision_config, save_decision_config};
pub use disk::{DiskSpace, NoDiskCheck};
pub use evaluate::{
    EvaluateRequest, EvaluatedRelease, Evaluation, ServiceError, evaluate_releases,
};
pub use formats::{input_for_file, input_for_history, input_for_release, matching_ids};
pub use grab::{GrabContext, GrabbedRelease, ProcessedDecisions, process_decisions};
pub use import::{
    FileOutcome, FileResult, ImportConfig, ImportMode, ImportRejection, ImportRequest,
    ImportResult, Importer, MediaProbe, ProbeInfo,
};
pub use snapshot::{DecisionSnapshot, QueueMap};
pub use sync::{
    IndexerFailure, IndexerPool, RssSyncResult, SearchMode, SearchResult, SyncContext, rss_sync,
    search_movie,
};
pub use tracking::{DownloadMonitor, MonitorOptions, RefreshResult, TrackedDownload, TrackedState};
