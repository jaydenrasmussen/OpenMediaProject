use std::{net::SocketAddr, path::PathBuf, time::Duration};

#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Databases, session data and (by default) downloads.
    pub data_dir: PathBuf,
    /// Where the built-in download clients put things unless told otherwise.
    pub downloads_dir: PathBuf,
    pub ui_dir: PathBuf,
    pub addr: SocketAddr,
    /// TMDB read access token; without one, adding movies is unavailable.
    pub tmdb_token: Option<String>,
    pub ffprobe: PathBuf,
    pub par2: PathBuf,
    pub sevenzip: PathBuf,
    /// Radarr's default RSS interval is 15 minutes.
    pub rss_interval: Duration,
    pub process_interval: Duration,
    /// How often the database is backed up (Radarr: weekly).
    pub backup_interval: Duration,
    /// How often movie details are re-read from the metadata provider.
    pub refresh_interval: Duration,
    /// Folders scanned for plugins; a plugin id in a later folder replaces the same id in an earlier one.
    pub plugin_dirs: Vec<PathBuf>,
    /// The category our downloads are filed under in the download clients.
    pub download_category: String,
}

impl AppConfig {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir: PathBuf = data_dir.into();
        Self {
            downloads_dir: data_dir.join("downloads"),
            ui_dir: PathBuf::from("ui/out"),
            addr: ([0, 0, 0, 0], 8080).into(),
            tmdb_token: None,
            ffprobe: "ffprobe".into(),
            par2: "par2".into(),
            sevenzip: "7zz".into(),
            rss_interval: Duration::from_secs(15 * 60),
            process_interval: Duration::from_secs(60),
            backup_interval: Duration::from_secs(7 * 24 * 3600),
            refresh_interval: Duration::from_secs(12 * 3600),
            plugin_dirs: vec![data_dir.join("plugins")],
            data_dir,
            download_category: "movies".into(),
        }
    }

    pub fn from_env() -> anyhow::Result<Self> {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let mut c = Self::new(var("OMP_DATA_DIR").unwrap_or_else(|| "data".into()));
        if let Some(v) = var("OMP_DOWNLOADS_DIR") {
            c.downloads_dir = v.into();
        }
        if let Some(v) = var("OMP_UI_DIR") {
            c.ui_dir = v.into();
        }
        if let Some(v) = var("OMP_ADDR") {
            c.addr = v.parse()?;
        }
        c.tmdb_token = var("TMDB_READ_TOKEN");
        if let Some(v) = var("OMP_FFPROBE") {
            c.ffprobe = v.into();
        }
        if let Some(v) = var("OMP_PAR2") {
            c.par2 = v.into();
        }
        if let Some(v) = var("OMP_7ZZ") {
            c.sevenzip = v.into();
        }
        // The image ships first-party plugins; the user's own live in the data folder.
        if let Some(v) = var("OMP_BUNDLED_PLUGINS_DIR") {
            c.plugin_dirs.insert(0, v.into());
        }
        if let Some(v) = var("OMP_PLUGINS_DIR") {
            c.plugin_dirs.push(v.into());
        }
        if let Some(v) = var("OMP_RSS_INTERVAL_MINUTES") {
            c.rss_interval = Duration::from_secs(v.parse::<u64>()?.max(1) * 60);
        }
        Ok(c)
    }
}
