//! Turning stored download client settings into running clients.

use std::{path::PathBuf, sync::Arc, time::Duration};

use omp_db::{
    Db,
    repo::{Provider, ProviderKind},
};
use omp_downloads::{ClientRegistry, DownloadClient, RegisteredClient};
use omp_torrent::{TorrentClient, TorrentSettings};
use omp_usenet::{ServerConfig, Tools, UsenetClient, UsenetSettings};
use serde::Deserialize;

use crate::config::AppConfig;

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct TorrentJson {
    download_dir: Option<PathBuf>,
    seed_ratio_limit: f64,
    enable_dht: Option<bool>,
    listen_port: Option<u16>,
    /// Minutes to keep seeding after finishing; 0 for no time goal.
    seed_time_limit_minutes: u64,
    /// KiB per second; 0 is unlimited.
    download_limit_kbps: u32,
    upload_limit_kbps: u32,
    /// Downloads at once; 0 is unlimited.
    max_active_downloads: usize,
    peer_limit: Option<usize>,
    bind_interface: Option<String>,
    ipv4_only: bool,
    enable_upnp: bool,
    enable_lsd: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct ServerJson {
    host: String,
    port: Option<u16>,
    tls: bool,
    username: Option<String>,
    password: Option<String>,
    connections: usize,
    priority: u32,
}

impl Default for ServerJson {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: None,
            tls: true,
            username: None,
            password: None,
            connections: 8,
            priority: 0,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct UsenetJson {
    servers: Vec<ServerJson>,
    incomplete_dir: Option<PathBuf>,
    complete_dir: Option<PathBuf>,
    /// Total speed limit in KiB/s; 0 is unlimited.
    speed_limit_kbps: u64,
}

/// The plugin client a provider names, if it does.
fn plugin_client(
    p: &Provider,
    plugins: &crate::plugins::Plugins,
    resolver: Option<Arc<dyn omp_downloads::LinkResolver>>,
) -> Option<Result<crate::plugin_client::PluginClient, String>> {
    let id = p.plugin_id.as_deref()?;
    Some(match (plugins.download_client(id), plugins.manifest(id)) {
        (Some(plugin), Some(m)) => Ok(crate::plugin_client::PluginClient::new(
            plugin,
            &p.settings,
            m.protocol.as_deref().unwrap_or("torrent"),
            resolver,
        )),
        _ => Err(format!("plugin '{id}' isn't installed")),
    })
}

async fn build_one(
    p: &Provider,
    config: &AppConfig,
    plugins: &crate::plugins::Plugins,
    resolver: Arc<dyn omp_downloads::LinkResolver>,
) -> Result<Arc<dyn DownloadClient>, String> {
    if let Some(client) = plugin_client(p, plugins, Some(resolver.clone())) {
        return Ok(Arc::new(client?));
    }
    match p.implementation.as_str() {
        "torrent" => {
            let j: TorrentJson =
                serde_json::from_value(p.settings.clone()).map_err(|e| e.to_string())?;
            let mut s = TorrentSettings::new(
                j.download_dir
                    .unwrap_or_else(|| config.downloads_dir.join("torrent")),
            );
            s.session_dir = Some(config.data_dir.join(format!("torrent-session-{}", p.id)));
            s.seed_ratio_limit = j.seed_ratio_limit;
            s.enable_dht = j.enable_dht.unwrap_or(true);
            s.listen_port = j.listen_port;
            s.seed_time_limit_minutes = j.seed_time_limit_minutes;
            s.download_limit_bps = j.download_limit_kbps.saturating_mul(1024);
            s.upload_limit_bps = j.upload_limit_kbps.saturating_mul(1024);
            s.max_active_downloads = j.max_active_downloads;
            s.peer_limit = j.peer_limit.filter(|n| *n > 0);
            s.bind_interface = j.bind_interface;
            s.ipv4_only = j.ipv4_only;
            s.enable_upnp = j.enable_upnp;
            s.enable_lsd = j.enable_lsd.unwrap_or(true);
            Ok(Arc::new(
                TorrentClient::start(s)
                    .await
                    .map_err(|e| e.to_string())?
                    .with_resolver(resolver),
            ))
        }
        "usenet" => {
            let j: UsenetJson =
                serde_json::from_value(p.settings.clone()).map_err(|e| e.to_string())?;
            if j.servers.is_empty() {
                return Err(NO_NEWS_SERVER.into());
            }
            let servers = j
                .servers
                .into_iter()
                .map(|s| {
                    let mut c =
                        ServerConfig::new(s.host, s.port.unwrap_or(if s.tls { 563 } else { 119 }));
                    c.tls = s.tls;
                    c.username = s.username;
                    c.password = s.password;
                    c.max_connections = s.connections.max(1);
                    c.priority = s.priority;
                    c.timeout = Duration::from_secs(30);
                    c
                })
                .collect();
            let root = config.data_dir.join(format!("usenet-{}", p.id));
            let downloads = config.downloads_dir.join("usenet");
            Ok(Arc::new(
                UsenetClient::start(UsenetSettings {
                    servers,
                    incomplete_dir: j
                        .incomplete_dir
                        .unwrap_or_else(|| downloads.join("incomplete")),
                    complete_dir: j.complete_dir.unwrap_or_else(|| downloads.join("complete")),
                    state_dir: root,
                    tools: Tools {
                        par2: config.par2.clone(),
                        sevenzip: config.sevenzip.clone(),
                    },
                    speed_limit_bps: j.speed_limit_kbps.saturating_mul(1024),
                })
                .await
                .map_err(|e| e.to_string())?,
            ))
        }
        other => Err(format!("download client type '{other}' is not built in")),
    }
}

const NO_NEWS_SERVER: &str = "no usenet servers configured";

/// Starts every enabled download client. One that fails to start is logged
/// and left out; the rest still work.
pub async fn build_registry(
    db: &Db,
    config: &AppConfig,
    plugins: &crate::plugins::Plugins,
    resolver: Arc<dyn omp_downloads::LinkResolver>,
) -> anyhow::Result<ClientRegistry> {
    let mut registered = Vec::new();
    for p in db.providers().list(ProviderKind::DownloadClient).await? {
        if !p.enabled {
            continue;
        }
        match build_one(&p, config, plugins, resolver.clone()).await {
            Ok(client) => registered.push(RegisteredClient {
                id: p.id,
                name: p.name.clone(),
                priority: p.priority,
                tags: p.tags.clone(),
                client,
            }),
            // Not having a news server yet is a step to do, not a fault (see health).
            Err(e) if e == NO_NEWS_SERVER => {
                tracing::info!(client = %p.name, "download client waits for a news server")
            }
            Err(e) => tracing::warn!(client = %p.name, "could not start download client: {e}"),
        }
    }
    Ok(ClientRegistry::new(registered))
}

/// Checks a download client's settings without starting it: usenet servers
/// are connected to and logged in to; the torrent folder must be writable.
pub async fn test_client(
    p: &Provider,
    config: &AppConfig,
    plugins: &crate::plugins::Plugins,
) -> Result<(), String> {
    if let Some(client) = plugin_client(p, plugins, None) {
        return client?.test().await;
    }
    match p.implementation.as_str() {
        "torrent" => {
            let j: TorrentJson =
                serde_json::from_value(p.settings.clone()).map_err(|e| e.to_string())?;
            let dir = j
                .download_dir
                .unwrap_or_else(|| config.downloads_dir.join("torrent"));
            tokio::fs::create_dir_all(&dir)
                .await
                .map_err(|e| format!("cannot use {}: {e}", dir.display()))?;
            let probe = dir.join(".omp-write-test");
            tokio::fs::write(&probe, b"")
                .await
                .map_err(|e| format!("{} is not writable: {e}", dir.display()))?;
            let _ = tokio::fs::remove_file(probe).await;
            Ok(())
        }
        "usenet" => {
            let j: UsenetJson =
                serde_json::from_value(p.settings.clone()).map_err(|e| e.to_string())?;
            if j.servers.is_empty() {
                return Err("add at least one usenet server".into());
            }
            for s in j.servers {
                let host = s.host.clone();
                let mut c =
                    ServerConfig::new(s.host, s.port.unwrap_or(if s.tls { 563 } else { 119 }));
                c.tls = s.tls;
                c.username = s.username;
                c.password = s.password;
                c.timeout = Duration::from_secs(10);
                omp_usenet::NntpConnection::connect(&c)
                    .await
                    .map_err(|e| format!("{host}: {e}"))?
                    .quit()
                    .await;
            }
            Ok(())
        }
        other => Err(format!("download client type '{other}' is not built in")),
    }
}
