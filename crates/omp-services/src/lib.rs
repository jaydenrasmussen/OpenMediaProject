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
mod recycle;
mod rename;
mod scan;
mod scene;
mod series;
mod snapshot;
mod sync;
mod tracking;
mod tv_evaluate;
mod tv_grab;
mod tv_import;
mod tv_library;
mod tv_match;
mod tv_sync;

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
    ImportResult, Importer, ManualImport, MediaProbe, ProbeInfo, list_media_files,
};
pub use media::naming_media;
pub use process::{ProcessOptions, ProcessResult, process_downloads};
pub use recycle::{clean_bin as clean_recycle_bin, dispose as dispose_file};
pub use rename::{RenamePreview, rename_movie, rename_preview, rename_previews};
pub use scan::{
    RescanResult, UnmappedFolder, rescan_movie, unmapped_folders, unmapped_series_folders,
};
pub use scene::{SceneRefresh, refresh_scene_data, refresh_scene_numbering, refresh_scene_titles};
pub use series::{
    AddSeries, RefreshStats, add_series, apply_monitor_mode, monitored_by_mode, refresh_series,
};
pub use snapshot::{DecisionSnapshot, QueueMap};
pub use sync::{
    IndexerFailure, IndexerPool, PluginIndexers, RssSyncResult, SearchMode, SearchResult,
    SyncContext, push_release, rss_sync, search_movie, test_indexer,
};
pub use tracking::{
    DownloadMonitor, MonitorOptions, RefreshResult, TrackedDownload, TrackedState, record_failure,
    record_tv_failure,
};
pub use tv_evaluate::{
    EvaluatedTvRelease, TvEvaluateRequest, TvEvaluation, TvQueueMap, TvSnapshot,
    evaluate_tv_releases, input_for_episode_file, input_for_episode_release,
};
pub use tv_grab::{grab_selected_tv, process_tv_decisions};
pub use tv_import::{
    TvImportRequest, TvImporter, match_files_to_episodes, naming_episode, naming_series,
};
pub use tv_library::{
    EpisodeRenamePreview, SeriesRescan, episode_rename_previews, rename_series, rescan_series,
};
pub use tv_match::{
    ReleaseIds, SeriesIndex, SeriesMatchType, TvMatch, find_series, match_release, pick_episodes,
    pick_scene_episodes,
};
pub use tv_sync::{
    SeriesSearchSummary, TvRssSyncResult, TvSearchResult, TvSyncContext, search_episode,
    search_missing, search_season, tv_rss_sync,
};
