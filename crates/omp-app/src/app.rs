use std::{
    path::Path,
    sync::{Arc, RwLock},
};

use chrono::{DateTime, Utc};
use omp_db::Db;
use omp_decision::DecisionEngine;
use omp_downloads::ClientRegistry;
use omp_metadata::{MovieMetadataProvider, tmdb::TmdbProvider};
use omp_services::{DiskSpace, DownloadMonitor, Ffprobe, MediaProbe};

use crate::{clients, config::AppConfig};

/// Free space from the operating system.
pub struct RealDisk;

impl DiskSpace for RealDisk {
    fn free_bytes(&self, path: &str) -> Option<u64> {
        // The folder may not exist yet; ask about the nearest one that does.
        Path::new(path)
            .ancestors()
            .find(|p| p.exists())
            .and_then(|p| fs4::available_space(p).ok())
    }
}

/// What a background job last did, for the Tasks page.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskStatus {
    pub name: &'static str,
    pub label: &'static str,
    pub interval_secs: u64,
    pub last_run: Option<DateTime<Utc>>,
    pub last_duration_ms: Option<u64>,
    pub last_error: Option<String>,
}

pub struct App {
    pub db: Db,
    pub config: AppConfig,
    pub engine: DecisionEngine,
    pub monitor: DownloadMonitor,
    pub metadata: Option<Arc<dyn MovieMetadataProvider>>,
    pub probe: Arc<dyn MediaProbe>,
    pub disk: Arc<dyn DiskSpace>,
    pub plugins: Arc<crate::plugins::Plugins>,
    pub started: DateTime<Utc>,
    /// A real password hash nobody knows the password to, checked when a login names an unknown user.
    pub(crate) dummy_hash: String,
    pub(crate) login_limiter: crate::auth::LoginLimiter,
    /// Interactive search results, kept so a chosen release can be grabbed.
    pub(crate) search_cache: std::sync::Mutex<crate::releases::SearchCache>,
    tasks: std::sync::Mutex<std::collections::BTreeMap<&'static str, TaskStatus>>,
    shutdown: tokio::sync::Notify,
    pub(crate) rss_lock: tokio::sync::Mutex<()>,
    pub(crate) process_lock: tokio::sync::Mutex<()>,
    registry: RwLock<Arc<ClientRegistry>>,
}

impl App {
    /// The real thing: databases on disk, TMDB, ffprobe, the OS's disk space.
    pub async fn open(config: AppConfig) -> anyhow::Result<Arc<Self>> {
        let db = Db::open(&config.data_dir).await?;
        let metadata = config
            .tmdb_token
            .clone()
            .map(|t| Arc::new(TmdbProvider::new(t)) as Arc<dyn MovieMetadataProvider>);
        let probe = Arc::new(Ffprobe::new(config.ffprobe.clone()));
        Self::new(config, db, metadata, probe, Arc::new(RealDisk)).await
    }

    /// Assembles an app from parts (tests pass fakes), seeds first-run
    /// defaults and starts the configured download clients.
    pub async fn new(
        config: AppConfig,
        db: Db,
        metadata: Option<Arc<dyn MovieMetadataProvider>>,
        probe: Arc<dyn MediaProbe>,
        disk: Arc<dyn DiskSpace>,
    ) -> anyhow::Result<Arc<Self>> {
        seed_defaults(&db).await?;
        let app = Arc::new(Self {
            db,
            engine: DecisionEngine::default(),
            monitor: DownloadMonitor::new(),
            metadata,
            probe,
            disk,
            plugins: Arc::new(crate::plugins::Plugins::load(&config.plugin_dirs)),
            config,
            started: Utc::now(),
            login_limiter: crate::auth::LoginLimiter::default(),
            dummy_hash: crate::auth::hash_password(&format!("{:?}", Utc::now()))
                .await
                .map_err(|e| anyhow::anyhow!("{}", e.1))?,
            search_cache: std::sync::Mutex::default(),
            tasks: std::sync::Mutex::default(),
            shutdown: tokio::sync::Notify::new(),
            rss_lock: tokio::sync::Mutex::new(()),
            process_lock: tokio::sync::Mutex::new(()),
            registry: RwLock::new(Arc::new(ClientRegistry::default())),
        });
        app.reload_clients().await?;
        Ok(app)
    }

    /// Makes a job appear on the Tasks page before it first runs.
    pub(crate) fn register_task(
        &self,
        name: &'static str,
        label: &'static str,
        every: std::time::Duration,
    ) {
        self.tasks
            .lock()
            .unwrap()
            .entry(name)
            .or_insert(TaskStatus {
                name,
                label,
                interval_secs: every.as_secs(),
                last_run: None,
                last_duration_ms: None,
                last_error: None,
            });
    }

    pub(crate) fn finish_task(
        &self,
        name: &'static str,
        started: DateTime<Utc>,
        error: Option<String>,
    ) {
        if let Some(t) = self.tasks.lock().unwrap().get_mut(name) {
            t.last_run = Some(started);
            t.last_duration_ms = Some((Utc::now() - started).num_milliseconds().max(0) as u64);
            t.last_error = error;
        }
    }

    pub fn task_statuses(&self) -> Vec<TaskStatus> {
        self.tasks.lock().unwrap().values().cloned().collect()
    }

    /// Asks the server to stop (the container's restart policy brings it
    /// back). Used after a restore.
    pub fn request_shutdown(&self) {
        self.shutdown.notify_one();
    }

    /// Resolves when [`Self::request_shutdown`] is called.
    pub async fn shutdown_requested(&self) {
        self.shutdown.notified().await;
    }

    /// The key programs use in the `X-Api-Key` header.
    pub async fn api_key(&self) -> anyhow::Result<String> {
        Ok(crate::auth::api_key(self).await?)
    }

    pub fn registry(&self) -> Arc<ClientRegistry> {
        self.registry.read().unwrap().clone()
    }

    /// Rebuilds the download clients from the stored settings. Call after
    /// they change.
    pub async fn reload_clients(&self) -> anyhow::Result<()> {
        let fresh = Arc::new(clients::build_registry(&self.db, &self.config).await?);
        let old = std::mem::replace(&mut *self.registry.write().unwrap(), fresh);
        for c in old.all() {
            c.client.shutdown().await;
        }
        Ok(())
    }

    pub async fn shutdown(&self) {
        for c in self.registry().all() {
            c.client.shutdown().await;
        }
    }
}

async fn seed_defaults(db: &Db) -> anyhow::Result<()> {
    db.delay_profiles().seed_default().await?;
    if db.quality_profiles().list().await?.is_empty() {
        for profile in omp_quality::defaults::default_profiles() {
            db.quality_profiles().insert(&profile).await?;
        }
    }
    Ok(())
}
