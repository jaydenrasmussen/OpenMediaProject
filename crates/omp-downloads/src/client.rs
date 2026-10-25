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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DownloadItemStatus {
    #[default]
    Queued,
    Paused,
    Downloading,
    Completed,
    Failed,
    Warning,
}

/// Where a download stands in its client's queue.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum QueuePriority {
    Low,
    #[default]
    Normal,
    High,
    /// Goes first and ignores the limit on active downloads.
    Force,
}

/// One download as the client reports it.
#[derive(Debug, Clone, PartialEq, Default)]
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
    /// Seconds until done at the current speed, when there is a speed.
    pub eta_seconds: Option<u64>,
    /// Bytes per second right now.
    pub download_speed: u64,
    pub priority: QueuePriority,
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

    /// Stops a download without losing what it has. Not every client can.
    async fn pause(&self, _download_id: &str) -> Result<(), ClientError> {
        Err(ClientError::Other(
            "this client cannot pause downloads".into(),
        ))
    }

    async fn resume(&self, _download_id: &str) -> Result<(), ClientError> {
        Err(ClientError::Other(
            "this client cannot resume downloads".into(),
        ))
    }

    async fn set_priority(
        &self,
        _download_id: &str,
        _priority: QueuePriority,
    ) -> Result<(), ClientError> {
        Err(ClientError::Other("this client has no priorities".into()))
    }

    /// Stops background work before the client is dropped or replaced.
    async fn shutdown(&self) {}
}

/// Links that begin with this name a release on a site the app reads itself
/// (`omp-indexer://<indexer id>?link=<address>`). A download client cannot
/// fetch those: the app signs in to the site and finds the file.
pub const INDEXER_LINK_PREFIX: &str = "omp-indexer://";

/// What such a link turns out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedLink {
    Magnet(String),
    Torrent(Vec<u8>),
}

#[async_trait]
pub trait LinkResolver: Send + Sync {
    async fn resolve(&self, link: &str) -> Result<ResolvedLink, String>;

    /// An address a client on another machine could fetch the torrent from.
    async fn proxy_url(&self, link: &str) -> Option<String>;
}
