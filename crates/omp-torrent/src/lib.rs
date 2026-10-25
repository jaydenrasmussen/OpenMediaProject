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
use omp_downloads::{
    AddRequest, ClientError, ClientItem, DownloadClient, DownloadItemStatus, INDEXER_LINK_PREFIX,
    LinkResolver, QueuePriority, ResolvedLink,
};

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
    /// Stop seeding after this many minutes. 0 means no time goal. With a
    /// ratio goal too, whichever is met first ends the seeding.
    pub seed_time_limit_minutes: u64,
    /// Bytes per second; 0 is unlimited.
    pub download_limit_bps: u32,
    pub upload_limit_bps: u32,
    /// Downloads running at once; 0 is no limit. The rest wait their turn.
    pub max_active_downloads: usize,
    pub peer_limit: Option<usize>,
    /// Bind to one network interface (a VPN, say).
    pub bind_interface: Option<String>,
    pub ipv4_only: bool,
    /// Ask the router to forward the listening port (UPnP).
    pub enable_upnp: bool,
    /// Find peers on the local network.
    pub enable_lsd: bool,
}

impl TorrentSettings {
    pub fn new(download_dir: impl Into<PathBuf>) -> Self {
        Self {
            download_dir: download_dir.into(),
            session_dir: None,
            enable_dht: true,
            listen_port: None,
            seed_ratio_limit: 0.0,
            seed_time_limit_minutes: 0,
            download_limit_bps: 0,
            upload_limit_bps: 0,
            max_active_downloads: 0,
            peer_limit: None,
            bind_interface: None,
            ipv4_only: false,
            enable_upnp: false,
            enable_lsd: true,
        }
    }
}

/// What the client remembers about a torrent beyond what the session knows.
#[derive(Default)]
struct Meta {
    priority: QueuePriority,
    /// Stopped by a person; the queue never restarts it.
    user_paused: bool,
    /// Stopped by the queue because too many downloads were running.
    queue_paused: bool,
    /// Order added, for first come first served.
    seq: u64,
    /// When we first saw it finished, for the seed time goal.
    finished_since: Option<std::time::Instant>,
    /// The indexer's goals, when it has any; they replace the client's.
    seed_ratio: Option<f64>,
    seed_time_minutes: Option<u64>,
}

pub struct TorrentClient {
    session: Arc<Session>,
    http: reqwest::Client,
    settings: TorrentSettings,
    meta: std::sync::Mutex<std::collections::HashMap<String, Meta>>,
    next_seq: std::sync::atomic::AtomicU64,
    resolver: std::sync::Mutex<Option<Arc<dyn LinkResolver>>>,
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
        options.ratelimits.download_bps = std::num::NonZeroU32::new(settings.download_limit_bps);
        options.ratelimits.upload_bps = std::num::NonZeroU32::new(settings.upload_limit_bps);
        options.peer_limit = settings.peer_limit;
        options.bind_device_name = settings.bind_interface.clone().filter(|b| !b.is_empty());
        options.ipv4_only = settings.ipv4_only;
        options.disable_local_service_discovery = !settings.enable_lsd;
        // TCP and uTP both; a port of 0 lets the system choose.
        options.listen = Some(ListenerOptions {
            mode: librqbit::ListenerMode::TcpAndUtp,
            listen_addr: if settings.ipv4_only {
                std::net::SocketAddr::from(([0, 0, 0, 0], settings.listen_port.unwrap_or(0)))
            } else {
                std::net::SocketAddr::from((
                    std::net::Ipv6Addr::UNSPECIFIED,
                    settings.listen_port.unwrap_or(0),
                ))
            },
            enable_upnp_port_forwarding: settings.enable_upnp,
            ipv4_only: settings.ipv4_only,
            ..Default::default()
        });
        let session = Session::new_with_opts(settings.download_dir.clone(), options)
            .await
            .map_err(|e| ClientError::Unavailable(e.to_string()))?;
        Ok(Self {
            session,
            http: omp_net::client_builder()
                .build()
                .expect("static client config"),
            settings,
            meta: Default::default(),
            next_seq: Default::default(),
            resolver: Default::default(),
        })
    }

    /// Lets the client follow links to sites the app reads itself.
    pub fn with_resolver(self, resolver: Arc<dyn LinkResolver>) -> Self {
        *self.resolver.lock().expect("resolver") = Some(resolver);
        self
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
        let resolver = self.resolver.lock().expect("resolver").clone();
        let add = if source.starts_with(INDEXER_LINK_PREFIX) {
            let resolver = resolver.ok_or_else(|| {
                ClientError::ReleaseUnavailable(
                    "this link needs the app's indexer to fetch it".into(),
                )
            })?;
            match resolver.resolve(source).await.map_err(|message| {
                ClientError::ReleaseDownloadFailed {
                    message,
                    retry_after: None,
                }
            })? {
                ResolvedLink::Magnet(m) => Source::Url(m),
                ResolvedLink::Torrent(b) => Source::Bytes(bytes::Bytes::from(b)),
            }
        } else if source.starts_with("http://") || source.starts_with("https://") {
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

impl TorrentClient {
    fn handle(&self, download_id: &str) -> Result<Arc<librqbit::ManagedTorrent>, ClientError> {
        let hash = download_id
            .parse::<librqbit::dht::Id20>()
            .map_err(|_| ClientError::Other("not a torrent id".into()))?;
        self.session
            .get(TorrentIdOrHash::Hash(hash))
            .ok_or_else(|| ClientError::Other("no such download".into()))
    }

    /// Done seeding: neither goal is set (do not seed at all), or the ratio
    /// or the time goal has been reached.
    fn seeding_goal_met(
        &self,
        indexer: (Option<f64>, Option<u64>),
        ratio: f64,
        seeding_minutes: u64,
    ) -> bool {
        // An indexer that names a goal sets both: one it leaves out is off.
        let (ratio_goal, time_goal) = if indexer.0.is_some() || indexer.1.is_some() {
            (indexer.0.unwrap_or(0.0), indexer.1.unwrap_or(0))
        } else {
            (
                self.settings.seed_ratio_limit,
                self.settings.seed_time_limit_minutes,
            )
        };
        if ratio_goal <= 0.0 && time_goal == 0 {
            return true;
        }
        (ratio_goal > 0.0 && ratio >= ratio_goal) || (time_goal > 0 && seeding_minutes >= time_goal)
    }

    /// Keeps at most `max_active_downloads` running, the most important
    /// first; forced downloads always run. The rest are paused by the queue
    /// and started again when a place frees up.
    async fn enforce_queue(&self, handles: &[Arc<librqbit::ManagedTorrent>]) {
        let limit = self.settings.max_active_downloads;
        if limit == 0 {
            return;
        }
        // Waiting downloads, best first.
        let mut waiting: Vec<(String, QueuePriority, u64, Arc<librqbit::ManagedTorrent>)> =
            Vec::new();
        {
            let meta = self.meta.lock().unwrap();
            for h in handles {
                let id = h.info_hash().as_string().to_uppercase();
                let m = meta.get(&id);
                let user_paused = m.is_some_and(|m| m.user_paused);
                if h.stats().finished || user_paused {
                    continue;
                }
                waiting.push((
                    id,
                    m.map(|m| m.priority).unwrap_or_default(),
                    m.map_or(u64::MAX, |m| m.seq),
                    h.clone(),
                ));
            }
        }
        waiting.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
        let mut running = 0;
        for (id, priority, _, handle) in waiting {
            let allowed = priority == QueuePriority::Force || running < limit;
            let paused = is_paused(&handle.stats().state);
            if allowed {
                // A forced download does not use up a place.
                if priority != QueuePriority::Force {
                    running += 1;
                }
                if paused
                    && self
                        .meta
                        .lock()
                        .unwrap()
                        .get(&id)
                        .is_some_and(|m| m.queue_paused)
                {
                    if let Err(e) = self.session.unpause(&handle).await {
                        tracing::warn!("could not start a queued torrent: {e:#}");
                    } else if let Some(m) = self.meta.lock().unwrap().get_mut(&id) {
                        m.queue_paused = false;
                    }
                }
            } else if !paused {
                if let Err(e) = self.session.pause(&handle).await {
                    tracing::warn!("could not hold back a torrent: {e:#}");
                } else {
                    self.meta
                        .lock()
                        .unwrap()
                        .entry(id)
                        .or_default()
                        .queue_paused = true;
                }
            }
        }
    }
}

/// Stopped, including a torrent that was stopped while still checking its files.
fn is_paused(state: &librqbit::TorrentStatsState) -> bool {
    matches!(
        state,
        librqbit::TorrentStatsState::Paused
            | librqbit::TorrentStatsState::Initializing { paused: true }
    )
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
        let goals = release.torrent.as_ref();
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
                let id = handle.info_hash().as_string().to_uppercase();
                let seq = self
                    .next_seq
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.meta
                    .lock()
                    .unwrap()
                    .entry(id.clone())
                    .or_insert_with(|| Meta {
                        seq,
                        seed_ratio: goals.and_then(|t| t.seed_ratio),
                        seed_time_minutes: goals.and_then(|t| t.seed_time_minutes).map(u64::from),
                        ..Default::default()
                    });
                Ok(id)
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
        self.enforce_queue(&handles).await;
        let mut items = Vec::new();
        for h in handles {
            let stats = h.stats();
            let name = h.name().unwrap_or_default();
            let id = h.info_hash().as_string().to_uppercase();
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
            let (priority, user_paused, queue_paused, seeding_minutes, indexer_goals) = {
                let mut meta = self.meta.lock().unwrap();
                let m = meta.entry(id.clone()).or_default();
                if stats.finished && m.finished_since.is_none() {
                    m.finished_since = Some(std::time::Instant::now());
                }
                (
                    m.priority,
                    m.user_paused,
                    m.queue_paused,
                    m.finished_since.map_or(0, |t| t.elapsed().as_secs() / 60),
                    (m.seed_ratio, m.seed_time_minutes),
                )
            };
            let goal_met =
                stats.finished && self.seeding_goal_met(indexer_goals, ratio, seeding_minutes);

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
            } else if queue_paused {
                (DownloadItemStatus::Queued, None)
            } else if user_paused {
                (DownloadItemStatus::Paused, None)
            } else {
                match stats.state {
                    s if is_paused(&s) => (DownloadItemStatus::Paused, None),
                    librqbit::TorrentStatsState::Initializing { .. } => {
                        (DownloadItemStatus::Queued, None)
                    }
                    _ => (DownloadItemStatus::Downloading, None),
                }
            };
            let remaining = stats.total_bytes.saturating_sub(stats.progress_bytes);
            let speed = stats
                .live
                .as_ref()
                .map_or(0, |l| l.download_speed.as_bytes());
            items.push(ClientItem {
                download_id: id,
                category: self.category_of(&save_root),
                title: name.clone(),
                total_size: stats.total_bytes,
                remaining_size: remaining,
                status,
                output_path: Some(output_path.to_string_lossy().into_owned()),
                seed_ratio: Some(ratio),
                can_move_files: goal_met,
                can_be_removed: goal_met,
                message,
                eta_seconds: (speed > 0 && remaining > 0).then(|| remaining / speed),
                download_speed: speed,
                priority,
            });
        }
        Ok(items)
    }

    async fn pause(&self, download_id: &str) -> Result<(), ClientError> {
        let handle = self.handle(download_id)?;
        self.meta
            .lock()
            .unwrap()
            .entry(download_id.to_uppercase())
            .or_default()
            .user_paused = true;
        if !is_paused(&handle.stats().state) {
            self.session.pause(&handle).await.map_err(other)?;
        }
        Ok(())
    }

    async fn resume(&self, download_id: &str) -> Result<(), ClientError> {
        let handle = self.handle(download_id)?;
        {
            let mut meta = self.meta.lock().unwrap();
            let m = meta.entry(download_id.to_uppercase()).or_default();
            m.user_paused = false;
            m.queue_paused = false;
        }
        if is_paused(&handle.stats().state) {
            self.session.unpause(&handle).await.map_err(other)?;
        }
        Ok(())
    }

    async fn set_priority(
        &self,
        download_id: &str,
        priority: QueuePriority,
    ) -> Result<(), ClientError> {
        self.handle(download_id)?;
        self.meta
            .lock()
            .unwrap()
            .entry(download_id.to_uppercase())
            .or_default()
            .priority = priority;
        Ok(())
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
