//! Services that tie the pure crates to storage: loading what the decision
//! engine needs, evaluating releases end to end, and (next) syncing and
//! searching.

mod config;
mod disk;
mod evaluate;
mod formats;
mod snapshot;

pub use config::{DECISION_CONFIG_KEY, load_decision_config, save_decision_config};
pub use disk::{DiskSpace, NoDiskCheck};
pub use evaluate::{
    EvaluateRequest, EvaluatedRelease, Evaluation, ServiceError, evaluate_releases,
};
pub use formats::{input_for_file, input_for_history, input_for_release, matching_ids};
pub use snapshot::{DecisionSnapshot, QueueMap};
