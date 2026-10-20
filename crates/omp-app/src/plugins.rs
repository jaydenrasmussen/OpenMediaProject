//! Plugins found on disk, compiled and ready to call.

use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use omp_plugin_host::{
    Fetcher, HttpRequest, HttpResponse, Indexer, Kind, List, Manifest, Notification, Plugin,
    PluginHost, discover,
};

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

pub struct Plugins {
    manifests: Vec<Manifest>,
    problems: Vec<String>,
    indexers: HashMap<String, Plugin<Indexer>>,
    notifications: HashMap<String, Plugin<Notification>>,
    lists: HashMap<String, Plugin<List>>,
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

    /// Loads every plugin under `dirs`. A plugin that fails to load is
    /// reported in [`Self::problems`]; the rest still work.
    pub fn load(dirs: &[PathBuf]) -> Self {
        let mut me = Self::none();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("OpenMediaProject/", env!("CARGO_PKG_VERSION")))
            .build();
        let host = client
            .map_err(anyhow::Error::from)
            .and_then(|client| PluginHost::new(Arc::new(ReqwestFetcher { client })));
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
                    .load_indexer(m.wasm_path(), m.permissions())
                    .map(|p| drop(me.indexers.insert(m.id.clone(), p))),
                Kind::Notification => host
                    .load_notification(m.wasm_path(), m.permissions())
                    .map(|p| drop(me.notifications.insert(m.id.clone(), p))),
                Kind::List => host
                    .load_list(m.wasm_path(), m.permissions())
                    .map(|p| drop(me.lists.insert(m.id.clone(), p))),
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

    pub fn indexer(&self, id: &str) -> Option<&Plugin<Indexer>> {
        self.indexers.get(id)
    }

    pub fn notification(&self, id: &str) -> Option<&Plugin<Notification>> {
        self.notifications.get(id)
    }

    pub fn list(&self, id: &str) -> Option<&Plugin<List>> {
        self.lists.get(id)
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
