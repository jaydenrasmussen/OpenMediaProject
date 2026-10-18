//! The built-in BitTorrent client: a [`DownloadClient`] over librqbit.
//!
//! Downloads go to `<download_dir>/<category>/`, so the category survives a
//! restart without any state of our own. A download's id is its upper case
//! info hash, the same id history and the queue use.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ListenerOptions, Session, SessionOptions,
    SessionPersistenceConfig, api::TorrentIdOrHash,
};
use omp_core::release::DownloadProtocol;
use omp_downloads::{AddRequest, ClientError, ClientItem, DownloadClient, DownloadItemStatus};

#[derive(Debug, Clone)]
pub struct TorrentSettings {
    pub download_dir: PathBuf,
    /// Where librqbit keeps its session (resume data). `None` keeps nothing.
    pub session_dir: Option<PathBuf>,
    pub enable_dht: bool,
    pub listen_port: Option<u16>,
    /// Stop seeding at this upload/download ratio. 0 means do not seed: the
    /// download counts as done and removable as soon as it is complete.
    pub seed_ratio_limit: f64,
}

impl TorrentSettings {
    pub fn new(download_dir: impl Into<PathBuf>) -> Self {
        Self {
            download_dir: download_dir.into(),
            session_dir: None,
            enable_dht: true,
            listen_port: None,
            seed_ratio_limit: 0.0,
        }
    }
}

pub struct TorrentClient {
    session: Arc<Session>,
    http: reqwest::Client,
    settings: TorrentSettings,
}

fn other(e: impl std::fmt::Display) -> ClientError {
    ClientError::Other(e.to_string())
}

impl TorrentClient {
    pub async fn start(settings: TorrentSettings) -> Result<Self, ClientError> {
        tokio::fs::create_dir_all(&settings.download_dir)
            .await
            .map_err(other)?;
        let mut options = SessionOptions {
            disable_trackers: false,
            ..Default::default()
        };
        if !settings.enable_dht {
            options.dht = None;
        }
        if let Some(dir) = &settings.session_dir {
            tokio::fs::create_dir_all(dir).await.map_err(other)?;
            options.fastresume = true;
            options.persistence = Some(SessionPersistenceConfig::Json {
                folder: Some(dir.clone()),
            });
        }
        if let Some(port) = settings.listen_port {
            options.listen = Some(ListenerOptions {
                listen_addr: std::net::SocketAddr::from(([0, 0, 0, 0], port)),
                ..Default::default()
            });
        }
        let session = Session::new_with_opts(settings.download_dir.clone(), options)
            .await
            .map_err(|e| ClientError::Unavailable(e.to_string()))?;
        Ok(Self {
            session,
            http: reqwest::Client::new(),
            settings,
        })
    }

    pub async fn stop(&self) {
        self.session.stop().await;
    }

    fn category_dir(&self, category: Option<&str>) -> PathBuf {
        match category.filter(|c| !c.is_empty()) {
            Some(c) => self.settings.download_dir.join(sanitize(c)),
            None => self.settings.download_dir.clone(),
        }
    }

    /// The category folder a torrent was saved under, if any.
    fn category_of(&self, save_root: &Path) -> Option<String> {
        let relative = save_root.strip_prefix(&self.settings.download_dir).ok()?;
        relative
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
    }

    /// Gets the torrent file (or magnet) and works out where its files go:
    /// `<category dir>/<name>/` for several files, `<category dir>/` for one.
    async fn prepare(&self, source: &str, dir: &Path) -> Result<(Source, PathBuf), ClientError> {
        let add = if source.starts_with("http://") || source.starts_with("https://") {
            let response = self
                .http
                .get(source)
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| ClientError::ReleaseDownloadFailed {
                    message: e.to_string(),
                    retry_after: None,
                })?;
            let bytes = response
                .bytes()
                .await
                .map_err(|e| ClientError::ReleaseDownloadFailed {
                    message: e.to_string(),
                    retry_after: None,
                })?;
            Source::Bytes(bytes)
        } else {
            Source::Url(source.to_owned())
        };

        let listing = self
            .session
            .add_torrent(
                add.to_add(),
                Some(AddTorrentOptions {
                    list_only: true,
                    ..Default::default()
                }),
            )
            .await
            .map_err(|e| ClientError::ReleaseDownloadFailed {
                message: format!("{e:#}"),
                retry_after: None,
            })?;
        let AddTorrentResponse::ListOnly(info) = listing else {
            return Err(ClientError::Other(
                "unexpected response to a listing".into(),
            ));
        };
        // With nothing specified librqbit saves a multi-file torrent in a folder
        // named after it and a single file straight into the download dir.
        let default = info.output_folder;
        let subfolder = default
            .strip_prefix(&self.settings.download_dir)
            .unwrap_or(Path::new(""));
        Ok((add, dir.join(subfolder)))
    }
}

/// A torrent file's bytes or a magnet/URL librqbit fetches itself.
enum Source {
    Bytes(bytes::Bytes),
    Url(String),
}

impl Source {
    fn to_add(&self) -> AddTorrent<'static> {
        match self {
            Source::Bytes(b) => AddTorrent::from_bytes(b.clone()),
            Source::Url(u) => AddTorrent::from_url(u.clone()),
        }
    }
}

/// A category is a single folder name.
fn sanitize(category: &str) -> String {
    category
        .replace(['/', '\\'], "_")
        .trim_matches('.')
        .to_owned()
}

#[async_trait]
impl DownloadClient for TorrentClient {
    fn protocol(&self) -> DownloadProtocol {
        DownloadProtocol::Torrent
    }

    async fn add(&self, request: &AddRequest<'_>) -> Result<String, ClientError> {
        let release = request.release;
        let source = release
            .torrent
            .as_ref()
            .and_then(|t| t.magnet_url.clone())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| release.download_url.clone());
        if source.is_empty() {
            return Err(ClientError::ReleaseUnavailable(
                "release has no download link".into(),
            ));
        }
        let dir = self.category_dir(request.category);
        tokio::fs::create_dir_all(&dir).await.map_err(other)?;

        let (add, output_folder) = self.prepare(&source, &dir).await?;
        let response = self
            .session
            .add_torrent(
                add.to_add(),
                Some(AddTorrentOptions {
                    output_folder: Some(output_folder.to_string_lossy().into_owned()),
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await
            .map_err(|e| ClientError::ReleaseDownloadFailed {
                message: format!("{e:#}"),
                retry_after: None,
            })?;
        match response {
            AddTorrentResponse::Added(_, handle)
            | AddTorrentResponse::AlreadyManaged(_, handle) => {
                Ok(handle.info_hash().as_string().to_uppercase())
            }
            AddTorrentResponse::ListOnly(_) => {
                Err(ClientError::Other("unexpected list-only response".into()))
            }
        }
    }

    async fn items(&self) -> Result<Vec<ClientItem>, ClientError> {
        let handles: Vec<_> = self
            .session
            .with_torrents(|it| it.map(|(_, h)| h.clone()).collect());
        let mut items = Vec::new();
        for h in handles {
            let stats = h.stats();
            let name = h.name().unwrap_or_default();
            let output_folder = h.output_folder().to_path_buf();
            let multi_file = h
                .with_metadata(|m| m.info.iter_file_details().count() > 1)
                .unwrap_or(false);
            let (output_path, save_root) = if multi_file {
                (
                    output_folder.clone(),
                    output_folder
                        .parent()
                        .unwrap_or(&output_folder)
                        .to_path_buf(),
                )
            } else {
                // librqbit puts a one-file torrent's file straight into the folder.
                let file = h
                    .with_metadata(|m| {
                        m.info
                            .iter_file_details()
                            .next()
                            .map(|d| d.filename.to_pathbuf())
                    })
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| PathBuf::from(&name));
                (output_folder.join(file), output_folder.clone())
            };
            let ratio = if stats.total_bytes > 0 {
                stats.uploaded_bytes as f64 / stats.total_bytes as f64
            } else {
                0.0
            };
            let limit = self.settings.seed_ratio_limit;
            let goal_met = stats.finished && (limit <= 0.0 || ratio >= limit);

            // Once the seeding goal is met, stop, so the files can be moved.
            if goal_met
                && matches!(stats.state, librqbit::TorrentStatsState::Live)
                && let Err(e) = self.session.pause(&h).await
            {
                tracing::warn!(torrent = %name, "could not pause finished torrent: {e:#}");
            }

            let (status, message) = if let Some(error) = stats.error.clone() {
                (DownloadItemStatus::Failed, Some(error))
            } else if stats.finished {
                (DownloadItemStatus::Completed, None)
            } else {
                match stats.state {
                    librqbit::TorrentStatsState::Initializing { .. } => {
                        (DownloadItemStatus::Queued, None)
                    }
                    librqbit::TorrentStatsState::Paused => (DownloadItemStatus::Paused, None),
                    _ => (DownloadItemStatus::Downloading, None),
                }
            };
            items.push(ClientItem {
                download_id: h.info_hash().as_string().to_uppercase(),
                category: self.category_of(&save_root),
                title: name.clone(),
                total_size: stats.total_bytes,
                remaining_size: stats.total_bytes.saturating_sub(stats.progress_bytes),
                status,
                output_path: Some(output_path.to_string_lossy().into_owned()),
                seed_ratio: Some(ratio),
                can_move_files: goal_met,
                can_be_removed: goal_met,
                message,
            });
        }
        Ok(items)
    }

    async fn shutdown(&self) {
        self.stop().await;
    }

    async fn remove(&self, download_id: &str, delete_data: bool) -> Result<(), ClientError> {
        let hash = download_id.parse::<librqbit::dht::Id20>().map_err(other)?;
        self.session
            .delete(TorrentIdOrHash::Hash(hash), delete_data)
            .await
            .map_err(other)
    }
}
