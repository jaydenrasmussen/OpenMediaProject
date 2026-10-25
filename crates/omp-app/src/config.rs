use std::{net::SocketAddr, path::PathBuf, time::Duration};

#[derive(Debug, Clone)]
pub struct TlsFiles {
    pub cert: PathBuf,
    pub key: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Databases, session data and (by default) downloads.
    pub data_dir: PathBuf,
    /// Where the built-in download clients put things unless told otherwise.
    pub downloads_dir: PathBuf,
    pub ui_dir: PathBuf,
    /// A UI built into the binary; used instead of `ui_dir` when set.
    pub embedded_ui: Option<crate::ui_files::EmbeddedUi>,
    /// Reverse-proxy prefix such as `/media`; empty serves from the root.
    pub url_base: String,
    /// Serve HTTPS with these files instead of plain HTTP.
    pub tls: Option<TlsFiles>,
    /// Set from the environment, so the saved settings do not override them.
    pub addr_pinned: bool,
    pub url_base_pinned: bool,
    pub addr: SocketAddr,
    /// TMDB read access token; without one, adding movies is unavailable.
    pub tmdb_token: Option<String>,
    /// Where TMDB is reached; only changed to point at a mirror or a test double.
    pub tmdb_base_url: Option<String>,
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
    /// How often import lists are read.
    pub list_interval: Duration,
    /// Folders scanned for plugins; a plugin id in a later folder replaces the same id in an earlier one.
    pub plugin_dirs: Vec<PathBuf>,
    /// The category our downloads are filed under in the download clients.
    pub download_category: String,
    /// Hosts the image cache may fetch pictures from.
    pub image_hosts: Vec<String>,
}

impl AppConfig {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir: PathBuf = data_dir.into();
        Self {
            downloads_dir: data_dir.join("downloads"),
            ui_dir: PathBuf::from("ui/out"),
            embedded_ui: None,
            url_base: String::new(),
            tls: None,
            addr_pinned: false,
            url_base_pinned: false,
            addr: ([0, 0, 0, 0], 8080).into(),
            tmdb_token: None,
            tmdb_base_url: None,
            ffprobe: "ffprobe".into(),
            par2: "par2".into(),
            sevenzip: "7zz".into(),
            rss_interval: Duration::from_secs(15 * 60),
            process_interval: Duration::from_secs(60),
            backup_interval: Duration::from_secs(7 * 24 * 3600),
            refresh_interval: Duration::from_secs(12 * 3600),
            list_interval: Duration::from_secs(6 * 3600),
            plugin_dirs: vec![data_dir.join("plugins")],
            data_dir,
            download_category: "movies".into(),
            image_hosts: vec!["image.tmdb.org".into(), "artworks.thetvdb.com".into()],
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
        if let Some(v) = var("OMP_URL_BASE") {
            c.url_base = crate::ui_files::normalize_base(&v);
            c.url_base_pinned = true;
        }
        if let Some(v) = var("OMP_ADDR") {
            c.addr = v.parse()?;
            c.addr_pinned = true;
        }
        c.tmdb_token = var("TMDB_READ_TOKEN");
        c.tmdb_base_url = var("OMP_TMDB_BASE_URL");
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
