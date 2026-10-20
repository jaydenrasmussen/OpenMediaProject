use std::time::Duration;

/// Why an indexer call failed, classified the way Radarr does so the caller
/// can pick the right back-off (see [`IndexerError::backoff`]).
#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    #[error("API key rejected or missing: {0}")]
    ApiKey(String),
    /// Newznab "Request limit reached".
    #[error("indexer API limit reached")]
    RequestLimit { retry_after: Option<Duration> },
    /// HTTP 429.
    #[error("too many requests")]
    TooManyRequests { retry_after: Option<Duration> },
    /// An `<error>` element the indexer returned.
    #[error("indexer error {code:?}: {message}")]
    Api { code: Option<i32>, message: String },
    #[error("unexpected HTTP status {0}")]
    Http(u16),
    #[error("the indexer answered with a web page; the site may be blocked or down")]
    Html,
    #[error("unsupported feed: {0}")]
    UnsupportedFeed(String),
    #[error("could not read the response: {0}")]
    Parse(String),
    /// Connection refused, DNS failure, timeout.
    #[error("network error: {0}")]
    Network(String),
    /// An indexer plugin reported a problem (or crashed).
    #[error("plugin error: {0}")]
    Plugin(String),
}

/// How a failure should be recorded in the provider's back-off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackoffPolicy {
    /// False for plain connection failures, which hold the escalation level.
    pub escalate: bool,
    /// A server-requested (or default) minimum wait.
    pub minimum: Option<Duration>,
}

/// Radarr waits at least an hour after being told to slow down, unless the
/// server says how long.
const DEFAULT_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(3600);

impl IndexerError {
    pub fn backoff(&self) -> BackoffPolicy {
        match self {
            IndexerError::RequestLimit { retry_after }
            | IndexerError::TooManyRequests { retry_after } => BackoffPolicy {
                escalate: true,
                minimum: Some(retry_after.unwrap_or(DEFAULT_RATE_LIMIT_BACKOFF)),
            },
            IndexerError::Network(_) => BackoffPolicy {
                escalate: false,
                minimum: None,
            },
            _ => BackoffPolicy {
                escalate: true,
                minimum: None,
            },
        }
    }
}
