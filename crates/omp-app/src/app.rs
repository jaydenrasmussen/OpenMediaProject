use std::{
    path::Path,
    sync::{Arc, RwLock},
};

use chrono::{DateTime, Utc};
use omp_db::Db;
use omp_decision::{DecisionEngine, tv::TvDecisionEngine};
use omp_downloads::ClientRegistry;
use omp_metadata::{MovieMetadataProvider, SeriesMetadataProvider, tmdb::TmdbProvider};
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
    pub tv_engine: TvDecisionEngine,
    pub events: crate::events::Events,
    pub monitor: DownloadMonitor,
    pub metadata: Option<Arc<dyn MovieMetadataProvider>>,
    pub series_metadata: Option<Arc<dyn SeriesMetadataProvider>>,
    pub scene_source: Option<Arc<dyn omp_metadata::scene::SceneSource>>,
    pub probe: Arc<dyn MediaProbe>,
    pub disk: Arc<dyn DiskSpace>,
    plugins: RwLock<Arc<crate::plugins::Plugins>>,
    /// Torrent sites read from Prowlarr's definitions.
    pub cardigann: Arc<crate::cardigann::Hub>,
    presence: RwLock<Arc<crate::media_servers::Presence>>,
    pub started: DateTime<Utc>,
    /// A real password hash nobody knows the password to, checked when a login names an unknown user.
    pub(crate) dummy_hash: String,
    pub(crate) login_limiter: crate::auth::LoginLimiter,
    /// Interactive search results, kept so a chosen release can be grabbed.
    pub(crate) search_cache: std::sync::Mutex<crate::releases::SearchCache>,
    pub(crate) tv_search_cache:
        std::sync::Mutex<crate::releases::SearchCache<omp_services::TvEvaluation>>,
    tasks: std::sync::Mutex<std::collections::BTreeMap<&'static str, TaskStatus>>,
    shutdown: tokio::sync::Notify,
    pub(crate) rss_lock: tokio::sync::Mutex<()>,
    pub(crate) tv_rss_lock: tokio::sync::Mutex<()>,
    pub(crate) process_lock: tokio::sync::Mutex<()>,
    registry: RwLock<Arc<ClientRegistry>>,
}

impl App {
    /// The real thing: databases on disk, TMDB, ffprobe, the OS's disk space.
    pub async fn open(mut config: AppConfig) -> anyhow::Result<Arc<Self>> {
        let db = Db::open(&config.data_dir).await?;
        // Saved port, URL base, HTTPS and proxy, before anything is built from them.
        crate::host_settings::apply(&db, &mut config).await;
        let tmdb = config.tmdb_token.clone().map(|t| {
            let tmdb = TmdbProvider::new(t);
            Arc::new(match &config.tmdb_base_url {
                Some(base) => tmdb.with_base_url(base.clone()),
                None => tmdb,
            })
        });
        let probe = Arc::new(Ffprobe::new(config.ffprobe.clone()));
        // Radarr's metadata server first (it needs no key), TMDB behind it.
        // OMP_RADARR_METADATA=off uses TMDB alone.
        let movies: Option<Arc<dyn MovieMetadataProvider>> = {
            let tmdb = tmdb.clone().map(|t| t as Arc<dyn MovieMetadataProvider>);
            let radarr_off = std::env::var("OMP_RADARR_METADATA")
                .is_ok_and(|v| matches!(v.as_str(), "off" | "0" | "false"));
            match (radarr_off, tmdb) {
                (true, tmdb) => tmdb,
                (false, None) => Some(Arc::new(omp_metadata::radarr::RadarrMetaProvider::new())),
                (false, Some(tmdb)) => Some(Arc::new(omp_metadata::radarr::Fallback::new(
                    Arc::new(omp_metadata::radarr::RadarrMetaProvider::new()),
                    tmdb,
                ))),
            }
        };
        Self::build(
            config,
            db,
            movies,
            tmdb.map(|t| {
                let tmdb = t as Arc<dyn SeriesMetadataProvider>;
                // SkyHook's TVDB episode lists are preferred; OMP_SKYHOOK=off uses TMDB alone.
                if std::env::var("OMP_SKYHOOK")
                    .is_ok_and(|v| matches!(v.as_str(), "off" | "0" | "false"))
                {
                    tmdb
                } else {
                    Arc::new(omp_metadata::skyhook::SkyhookSeriesProvider::new(tmdb))
                        as Arc<dyn SeriesMetadataProvider>
                }
            }),
            Some(Arc::new(omp_metadata::scene::HttpSceneSource::new())),
            probe,
            Arc::new(RealDisk),
        )
        .await
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
        Self::with_series_metadata(config, db, metadata, None, probe, disk).await
    }

    /// As [`Self::new`], with a provider for TV series as well.
    pub async fn with_series_metadata(
        config: AppConfig,
        db: Db,
        metadata: Option<Arc<dyn MovieMetadataProvider>>,
        series_metadata: Option<Arc<dyn SeriesMetadataProvider>>,
        probe: Arc<dyn MediaProbe>,
        disk: Arc<dyn DiskSpace>,
    ) -> anyhow::Result<Arc<Self>> {
        Self::build(config, db, metadata, series_metadata, None, probe, disk).await
    }

    /// Everything, including where scene numbering comes from (`None` keeps
    /// the app off the network for it).
    pub async fn build(
        config: AppConfig,
        db: Db,
        metadata: Option<Arc<dyn MovieMetadataProvider>>,
        series_metadata: Option<Arc<dyn SeriesMetadataProvider>>,
        scene_source: Option<Arc<dyn omp_metadata::scene::SceneSource>>,
        probe: Arc<dyn MediaProbe>,
        disk: Arc<dyn DiskSpace>,
    ) -> anyhow::Result<Arc<Self>> {
        seed_defaults(&db).await?;
        let cardigann = Arc::new(crate::cardigann::Hub::new(db.clone(), &config));
        let app = Arc::new(Self {
            db,
            engine: DecisionEngine::default(),
            tv_engine: TvDecisionEngine::default(),
            events: crate::events::Events::new(),
            monitor: DownloadMonitor::new(),
            metadata,
            series_metadata,
            scene_source,
            probe,
            disk,
            plugins: RwLock::new(Arc::new(crate::plugins::Plugins::load(&config.plugin_dirs))),
            cardigann,
            presence: RwLock::default(),
            config,
            started: Utc::now(),
            login_limiter: crate::auth::LoginLimiter::default(),
            dummy_hash: crate::auth::hash_password(&format!("{:?}", Utc::now()))
                .await
                .map_err(|e| anyhow::anyhow!("{}", e.1))?,
            search_cache: std::sync::Mutex::default(),
            tv_search_cache: std::sync::Mutex::default(),
            tasks: std::sync::Mutex::default(),
            shutdown: tokio::sync::Notify::new(),
            rss_lock: tokio::sync::Mutex::new(()),
            tv_rss_lock: tokio::sync::Mutex::new(()),
            process_lock: tokio::sync::Mutex::new(()),
            registry: RwLock::new(Arc::new(ClientRegistry::default())),
        });
        app.reload_clients().await?;
        app.refresh_presence().await;
        app.cardigann.load_solver().await;
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

    /// What runs indexers that are not Newznab or Torznab: plugins, and the
    /// torrent sites read from their definitions.
    pub fn indexer_runner(&self) -> Arc<crate::cardigann::Runner> {
        Arc::new(crate::cardigann::Runner {
            plugins: self.plugins(),
            hub: self.cardigann.clone(),
        })
    }

    /// The plugins that are loaded right now.
    pub fn plugins(&self) -> Arc<crate::plugins::Plugins> {
        self.plugins.read().unwrap().clone()
    }

    /// Loads the plugins again from disk (after one was installed or removed).
    pub async fn reload_plugins(&self) {
        let dirs = self.config.plugin_dirs.clone();
        let fresh = tokio::task::spawn_blocking(move || crate::plugins::Plugins::load(&dirs))
            .await
            .map(Arc::new);
        match fresh {
            Ok(fresh) => *self.plugins.write().unwrap() = fresh,
            Err(e) => tracing::warn!("reloading plugins failed: {e}"),
        }
    }

    /// What the media servers held at their last sync.
    pub fn presence(&self) -> Arc<crate::media_servers::Presence> {
        self.presence.read().unwrap().clone()
    }

    /// Rebuilds [`Self::presence`] from the database.
    pub async fn refresh_presence(&self) {
        match crate::media_servers::Presence::build(self).await {
            Ok(p) => *self.presence.write().unwrap() = Arc::new(p),
            Err(e) => tracing::warn!("reading the media server library failed: {e}"),
        }
    }

    pub fn registry(&self) -> Arc<ClientRegistry> {
        self.registry.read().unwrap().clone()
    }

    /// Swaps in a ready-made set of download clients (tests use fakes).
    pub fn replace_registry(&self, registry: ClientRegistry) {
        *self.registry.write().unwrap() = Arc::new(registry);
    }

    /// Rebuilds the download clients from the stored settings. Call after
    /// they change.
    pub async fn reload_clients(&self) -> anyhow::Result<()> {
        let fresh = Arc::new(
            clients::build_registry(
                &self.db,
                &self.config,
                &self.plugins(),
                self.cardigann.clone(),
            )
            .await?,
        );
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
