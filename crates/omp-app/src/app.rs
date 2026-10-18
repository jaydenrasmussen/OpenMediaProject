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

pub struct App {
    pub db: Db,
    pub config: AppConfig,
    pub engine: DecisionEngine,
    pub monitor: DownloadMonitor,
    pub metadata: Option<Arc<dyn MovieMetadataProvider>>,
    pub probe: Arc<dyn MediaProbe>,
    pub disk: Arc<dyn DiskSpace>,
    pub started: DateTime<Utc>,
    /// A real password hash nobody knows the password to, checked when a login names an unknown user.
    pub(crate) dummy_hash: String,
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
            config,
            engine: DecisionEngine::default(),
            monitor: DownloadMonitor::new(),
            metadata,
            probe,
            disk,
            started: Utc::now(),
            dummy_hash: crate::auth::hash_password(&format!("{:?}", Utc::now()))
                .await
                .map_err(|e| anyhow::anyhow!("{}", e.1))?,
            rss_lock: tokio::sync::Mutex::new(()),
            process_lock: tokio::sync::Mutex::new(()),
            registry: RwLock::new(Arc::new(ClientRegistry::default())),
        });
        app.reload_clients().await?;
        Ok(app)
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
