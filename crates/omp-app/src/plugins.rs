//! Plugins found on disk, compiled and ready to call.

use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use chrono::{TimeZone, Utc};
use omp_core::release::{DownloadProtocol, ReleaseInfo, TorrentRelease};
use omp_db::Db;
use omp_db::repo::Provider;
use omp_indexers::MovieSearch;
use omp_plugin_host::{
    Fetcher, HttpRequest, HttpResponse, Indexer, Kind, List, Manifest, Notification, Plugin,
    PluginHost, Protocol, SearchQuery, Storage, discover,
};
use omp_services::PluginIndexers;

/// Plugin HTTP goes through one client: timeouts, a size cap, and no
/// redirects (a redirect could leave the hosts a plugin declared).
struct ReqwestFetcher {
    client: reqwest::Client,
}

const MAX_BODY: usize = 16 * 1024 * 1024;

#[async_trait::async_trait]
impl Fetcher for ReqwestFetcher {
    async fn request(&self, r: HttpRequest) -> anyhow::Result<HttpResponse> {
        let method = reqwest::Method::from_bytes(r.method.as_bytes())?;
        let mut req = self.client.request(method, &r.url);
        for (k, v) in &r.headers {
            req = req.header(k, v);
        }
        if let Some(body) = r.body {
            req = req.body(body);
        }
        // Errors carry the URL, which may hold a key; say what went wrong without it.
        let response = req
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("request failed: {}", e.without_url()))?;
        let status = response.status().as_u16();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| anyhow::anyhow!("reading the response failed: {}", e.without_url()))?;
        if bytes.len() > MAX_BODY {
            anyhow::bail!("the response is larger than {} MB", MAX_BODY / 1024 / 1024);
        }
        Ok(HttpResponse {
            status,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        })
    }
}

/// Backs plugin storage with the app's own database, in the `plugin_storage`
/// table (one row per plugin id and key; see `omp-db`'s `providers` table
/// for the equivalent per-provider pattern this follows).
struct DbStorage(Db);

#[async_trait::async_trait]
impl Storage for DbStorage {
    async fn get(&self, plugin_id: &str, key: &str) -> anyhow::Result<Option<String>> {
        Ok(self.0.plugin_storage().get(plugin_id, key).await?)
    }

    async fn set(&self, plugin_id: &str, key: &str, value: &str) -> anyhow::Result<()> {
        self.0.plugin_storage().set(plugin_id, key, value).await?;
        Ok(())
    }

    async fn delete(&self, plugin_id: &str, key: &str) -> anyhow::Result<()> {
        self.0.plugin_storage().delete(plugin_id, key).await?;
        Ok(())
    }
}

pub struct Plugins {
    manifests: Vec<Manifest>,
    problems: Vec<String>,
    indexers: HashMap<String, Arc<Plugin<Indexer>>>,
    notifications: HashMap<String, Arc<Plugin<Notification>>>,
    lists: HashMap<String, Arc<Plugin<List>>>,
}

impl Plugins {
    pub fn none() -> Self {
        Self {
            manifests: Vec::new(),
            problems: Vec::new(),
            indexers: HashMap::new(),
            notifications: HashMap::new(),
            lists: HashMap::new(),
        }
    }

    /// Loads every plugin under `dirs`, with per-plugin storage backed by
    /// `db`. A plugin that fails to load is reported in [`Self::problems`];
    /// the rest still work.
    pub fn load(dirs: &[PathBuf], db: Db) -> Self {
        let mut me = Self::none();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("OpenMediaProject/", env!("CARGO_PKG_VERSION")))
            .build();
        let host = client.map_err(anyhow::Error::from).and_then(|client| {
            PluginHost::with_storage(Arc::new(ReqwestFetcher { client }), Arc::new(DbStorage(db)))
        });
        let host = match host {
            Ok(h) => h,
            Err(e) => {
                me.problems
                    .push(format!("plugin runtime unavailable: {e:#}"));
                return me;
            }
        };

        // Later folders win: a user's copy of a plugin replaces the bundled one.
        let mut manifests: Vec<Manifest> = Vec::new();
        for dir in dirs {
            let (found, problems) = discover(dir);
            me.problems.extend(problems);
            for m in found {
                manifests.retain(|existing| existing.id != m.id);
                manifests.push(m);
            }
        }
        for m in manifests {
            let loaded = match m.kind {
                Kind::Indexer => host
                    .load_indexer(m.wasm_path(), &m.id, m.permissions())
                    .map(|p| drop(me.indexers.insert(m.id.clone(), Arc::new(p)))),
                Kind::Notification => host
                    .load_notification(m.wasm_path(), &m.id, m.permissions())
                    .map(|p| drop(me.notifications.insert(m.id.clone(), Arc::new(p)))),
                Kind::List => host
                    .load_list(m.wasm_path(), &m.id, m.permissions())
                    .map(|p| drop(me.lists.insert(m.id.clone(), Arc::new(p)))),
            };
            match loaded {
                Ok(()) => me.manifests.push(m),
                Err(e) => me.problems.push(format!("{}: {e:#}", m.id)),
            }
        }
        me
    }

    pub fn manifests(&self) -> &[Manifest] {
        &self.manifests
    }

    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    pub fn manifest(&self, id: &str) -> Option<&Manifest> {
        self.manifests.iter().find(|m| m.id == id)
    }

    pub fn indexer(&self, id: &str) -> Option<Arc<Plugin<Indexer>>> {
        self.indexers.get(id).cloned()
    }

    pub fn notification(&self, id: &str) -> Option<Arc<Plugin<Notification>>> {
        self.notifications.get(id).cloned()
    }

    pub fn list(&self, id: &str) -> Option<Arc<Plugin<List>>> {
        self.lists.get(id).cloned()
    }

    /// The JSON Schema of a plugin's settings.
    pub async fn settings_schema(&self, id: &str) -> Option<anyhow::Result<String>> {
        if let Some(p) = self.indexers.get(id) {
            return Some(p.settings_schema().await);
        }
        if let Some(p) = self.notifications.get(id) {
            return Some(p.settings_schema().await);
        }
        if let Some(p) = self.lists.get(id) {
            return Some(p.settings_schema().await);
        }
        None
    }
}

fn to_release(r: omp_plugin_host::Release, provider: &Provider) -> ReleaseInfo {
    let torrent = (r.protocol == Protocol::Torrent).then(|| TorrentRelease {
        magnet_url: r
            .download_url
            .starts_with("magnet:")
            .then(|| r.download_url.clone()),
        info_hash: r.info_hash.clone(),
        seeders: r.seeders.map(|s| s as i32),
        peers: r
            .seeders
            .zip(r.leechers)
            .map(|(s, l)| (s + l) as i32)
            .or(r.leechers.map(|l| l as i32)),
    });
    ReleaseInfo {
        guid: r.guid,
        title: r.title,
        size: r.size,
        download_url: r.download_url,
        indexer_id: provider.id as i32,
        indexer: provider.name.clone(),
        indexer_priority: provider.priority,
        protocol: match r.protocol {
            Protocol::Usenet => DownloadProtocol::Usenet,
            Protocol::Torrent => DownloadProtocol::Torrent,
        },
        publish_date: Utc
            .timestamp_opt(r.publish_date, 0)
            .single()
            .unwrap_or_else(Utc::now),
        container: None,
        indexer_flags: Default::default(),
        imdb_id: None,
        tmdb_id: 0,
        torrent,
    }
}

#[async_trait::async_trait]
impl PluginIndexers for Plugins {
    fn has(&self, plugin_id: &str) -> bool {
        self.indexers.contains_key(plugin_id)
    }

    async fn recent(&self, provider: &Provider) -> Result<Vec<ReleaseInfo>, String> {
        let plugin = self
            .indexer(provider.plugin_id.as_deref().unwrap_or_default())
            .ok_or("the plugin isn't installed")?;
        let found = plugin
            .recent(&provider.settings.to_string())
            .await
            .map_err(|e| format!("{e:#}"))?;
        Ok(found.into_iter().map(|r| to_release(r, provider)).collect())
    }

    async fn search(
        &self,
        provider: &Provider,
        query: &MovieSearch,
    ) -> Result<Vec<ReleaseInfo>, String> {
        let plugin = self
            .indexer(provider.plugin_id.as_deref().unwrap_or_default())
            .ok_or("the plugin isn't installed")?;
        let q = SearchQuery {
            term: query.titles.first().cloned().unwrap_or_default(),
            imdb_id: query.imdb_id.clone(),
            tmdb_id: Some(query.tmdb_id),
            year: u32::try_from(query.year).ok().filter(|y| *y > 0),
        };
        let found = plugin
            .search(&q, &provider.settings.to_string())
            .await
            .map_err(|e| format!("{e:#}"))?;
        Ok(found.into_iter().map(|r| to_release(r, provider)).collect())
    }
}
