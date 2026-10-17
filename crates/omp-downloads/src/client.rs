use std::time::Duration;

use async_trait::async_trait;
use omp_core::release::{DownloadProtocol, ReleaseInfo};

/// Why a client call failed, classified the way Radarr does.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ClientError {
    /// The client cannot be reached or is not configured. The release should
    /// wait in the pending queue and the client is backed off.
    #[error("download client unavailable: {0}")]
    Unavailable(String),
    #[error("download client rejected our credentials: {0}")]
    Authentication(String),
    /// The release is gone (expired NZB, dead torrent link).
    #[error("release unavailable: {0}")]
    ReleaseUnavailable(String),
    /// The release could not be fetched from the indexer; carries a
    /// server-requested wait for the indexer's back-off.
    #[error("could not download the release: {message}")]
    ReleaseDownloadFailed {
        message: String,
        retry_after: Option<Duration>,
    },
    /// The client refused this release (duplicate, blocked, invalid).
    #[error("download client rejected the release: {0}")]
    Rejected(String),
    #[error("{0}")]
    Other(String),
}

impl ClientError {
    /// Failures that say something about the client itself.
    pub fn is_client_failure(&self) -> bool {
        matches!(
            self,
            ClientError::Unavailable(_) | ClientError::Authentication(_)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadItemStatus {
    Queued,
    Paused,
    Downloading,
    Completed,
    Failed,
    Warning,
}

/// One download as the client reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientItem {
    pub download_id: String,
    pub category: Option<String>,
    pub title: String,
    pub total_size: u64,
    pub remaining_size: u64,
    pub status: DownloadItemStatus,
    /// Where the finished files are.
    pub output_path: Option<String>,
    pub seed_ratio: Option<f64>,
    /// Files may be moved (not hardlinked) because nothing is seeding.
    pub can_move_files: bool,
    /// The client is done with it and it can be removed.
    pub can_be_removed: bool,
    pub message: Option<String>,
}

pub struct AddRequest<'a> {
    pub release: &'a ReleaseInfo,
    /// The category (or label) to put the download in.
    pub category: Option<&'a str>,
}

#[async_trait]
pub trait DownloadClient: Send + Sync {
    fn protocol(&self) -> DownloadProtocol;

    /// Hands a release to the client. Returns the client's id for it, which
    /// is how history, the queue and imports find it again.
    async fn add(&self, request: &AddRequest<'_>) -> Result<String, ClientError>;

    async fn items(&self) -> Result<Vec<ClientItem>, ClientError>;

    async fn remove(&self, download_id: &str, delete_data: bool) -> Result<(), ClientError>;
}
