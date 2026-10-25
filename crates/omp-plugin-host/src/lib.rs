//! WASM plugin runtime.
//!
//! Plugins are WASI components (see `wit/omp-plugin.wit`). They get no ambient
//! network or filesystem access: the only way out is the `host` interface,
//! which checks every call against the plugin's declared permissions.

mod manifest;

use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, anyhow};
use wasmtime::{
    Config, Engine, Store,
    component::{Component, Linker, ResourceTable},
};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

pub use manifest::{Kind, Manifest, discover};

mod indexer_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "indexer-plugin",
        imports: { default: async },
        exports: { default: async },
    });
}

mod notification_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "notification-plugin",
        imports: { default: async },
        exports: { default: async },
        with: { "omp:plugin/host": crate::indexer_bindings::omp::plugin::host },
    });
}

mod list_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "list-plugin",
        imports: { default: async },
        exports: { default: async },
        with: { "omp:plugin/host": crate::indexer_bindings::omp::plugin::host },
    });
}

mod download_client_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "download-client-plugin",
        imports: { default: async },
        exports: { default: async },
        with: { "omp:plugin/host": crate::indexer_bindings::omp::plugin::host },
    });
}

mod media_server_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "media-server-plugin",
        imports: { default: async },
        exports: { default: async },
        with: { "omp:plugin/host": crate::indexer_bindings::omp::plugin::host },
    });
}

use download_client_bindings::{
    DownloadClientPlugin as DownloadClientWorld, exports::omp::plugin::download_client as dc,
};
use indexer_bindings::{
    IndexerPlugin as IndexerWorld, exports::omp::plugin::indexer as ix, omp::plugin::host,
};
use list_bindings::{ListPlugin as ListWorld, exports::omp::plugin::import_list as ls};
use media_server_bindings::{
    MediaServerPlugin as MediaServerWorld, exports::omp::plugin::media_server as ms,
};
use notification_bindings::{
    NotificationPlugin as NotificationWorld, exports::omp::plugin::notification as nt,
};

pub use dc::{AddRequest as ClientAddRequest, Item as ClientItemInfo, Status as ClientStatus};
pub use ix::{Protocol, Release, SearchQuery};
pub use ls::{Item as ListItem, ItemKind as ListItemKind};
pub use ms::{ItemKind as MediaItemKind, LibraryItem as MediaLibraryItem, Page as MediaPage};
pub use nt::{Event, MovieInfo};

/// One HTTP request a plugin asked for.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    /// Names in lower case; a repeated header repeats here.
    pub headers: Vec<(String, String)>,
}

/// Does the actual HTTP work for plugins. The host owns this so it can add
/// rate limiting, caching and proxy settings in one place.
#[async_trait::async_trait]
pub trait Fetcher: Send + Sync {
    async fn request(&self, request: HttpRequest) -> Result<HttpResponse>;
}

/// What a plugin declared it needs (from `plugin.toml`).
#[derive(Debug, Clone, Default)]
pub struct Permissions {
    /// Exact hostnames the plugin may call, or `*` for any.
    pub allowed_hosts: Vec<String>,
}

impl Permissions {
    fn allows(&self, host: &str) -> bool {
        self.allowed_hosts.iter().any(|h| h == "*" || h == host)
    }
}

/// Fuel given to each call. Bounds CPU use so a looping plugin cannot hang the host.
const FUEL_PER_CALL: u64 = 500_000_000;
/// Memory a plugin instance may grow to.
const MAX_MEMORY: usize = 256 * 1024 * 1024;
/// Web requests one call may make. Fuel does not count time spent waiting
/// on the network, so this bounds a plugin that loops over requests.
const MAX_HTTP_CALLS: u32 = 100;

struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    permissions: Permissions,
    fetcher: Arc<dyn Fetcher>,
    limits: wasmtime::StoreLimits,
    http_calls: u32,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl State {
    fn check(&mut self, url: &str) -> Result<(), String> {
        self.http_calls += 1;
        if self.http_calls > MAX_HTTP_CALLS {
            return Err(format!(
                "too many web requests in one call (the limit is {MAX_HTTP_CALLS})"
            ));
        }
        let parsed = url::Url::parse(url).map_err(|e| format!("invalid url: {e}"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(format!(
                "only http and https are allowed, not {}",
                parsed.scheme()
            ));
        }
        let host = parsed.host_str().unwrap_or_default();
        if self.permissions.allows(host) {
            Ok(())
        } else {
            Err(format!("host {host:?} is not allowed for this plugin"))
        }
    }
}

impl host::Host for State {
    async fn log(&mut self, level: host::LogLevel, message: String) {
        match level {
            host::LogLevel::Debug => tracing::debug!(target: "plugin", "{message}"),
            host::LogLevel::Info => tracing::info!(target: "plugin", "{message}"),
            host::LogLevel::Warn => tracing::warn!(target: "plugin", "{message}"),
            host::LogLevel::Error => tracing::error!(target: "plugin", "{message}"),
        }
    }

    async fn http(&mut self, request: host::HttpRequest) -> Result<host::HttpResponse, String> {
        self.check(&request.url)?;
        let method = request.method.to_ascii_uppercase();
        if !matches!(
            method.as_str(),
            "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD"
        ) {
            return Err(format!("unsupported method {method}"));
        }
        let response = self
            .fetcher
            .request(HttpRequest {
                method,
                url: request.url,
                headers: request.headers,
                body: request.body,
            })
            .await
            .map_err(|e| e.to_string())?;
        Ok(host::HttpResponse {
            status: response.status,
            body: response.body,
            headers: response.headers,
        })
    }

    async fn http_get(&mut self, url: String) -> Result<String, String> {
        self.check(&url)?;
        let response = self
            .fetcher
            .request(HttpRequest {
                method: "GET".into(),
                url,
                headers: Vec::new(),
                body: None,
            })
            .await
            .map_err(|e| e.to_string())?;
        if (200..300).contains(&response.status) {
            Ok(response.body)
        } else {
            Err(format!("the server answered {}", response.status))
        }
    }
}

/// Compiles plugins and runs them. Cheap to clone.
#[derive(Clone)]
pub struct PluginHost {
    engine: Engine,
    linker: Linker<State>,
    fetcher: Arc<dyn Fetcher>,
}

impl PluginHost {
    pub fn new(fetcher: Arc<dyn Fetcher>) -> Result<Self> {
        let mut config = Config::new();
        config.consume_fuel(true);
        let engine = Engine::new(&config)?;

        let mut linker = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
        host::add_to_linker::<State, wasmtime::component::HasSelf<State>>(&mut linker, |s| s)?;

        Ok(Self {
            engine,
            linker,
            fetcher,
        })
    }

    fn component(&self, wasm: &Path) -> Result<Component> {
        Component::from_file(&self.engine, wasm)
            .map_err(wt)
            .with_context(|| format!("loading plugin {}", wasm.display()))
    }

    pub fn load_indexer(
        &self,
        wasm: impl AsRef<Path>,
        permissions: Permissions,
    ) -> Result<Plugin<Indexer>> {
        Ok(Plugin::new(
            self.clone(),
            self.component(wasm.as_ref())?,
            permissions,
        ))
    }

    pub fn load_notification(
        &self,
        wasm: impl AsRef<Path>,
        permissions: Permissions,
    ) -> Result<Plugin<Notification>> {
        Ok(Plugin::new(
            self.clone(),
            self.component(wasm.as_ref())?,
            permissions,
        ))
    }

    pub fn load_list(
        &self,
        wasm: impl AsRef<Path>,
        permissions: Permissions,
    ) -> Result<Plugin<List>> {
        Ok(Plugin::new(
            self.clone(),
            self.component(wasm.as_ref())?,
            permissions,
        ))
    }

    pub fn load_download_client(
        &self,
        wasm: impl AsRef<Path>,
        permissions: Permissions,
    ) -> Result<Plugin<DownloadClient>> {
        Ok(Plugin::new(
            self.clone(),
            self.component(wasm.as_ref())?,
            permissions,
        ))
    }

    pub fn load_media_server(
        &self,
        wasm: impl AsRef<Path>,
        permissions: Permissions,
    ) -> Result<Plugin<MediaServer>> {
        Ok(Plugin::new(
            self.clone(),
            self.component(wasm.as_ref())?,
            permissions,
        ))
    }

    fn store(&self, permissions: &Permissions) -> Result<Store<State>> {
        let mut store = Store::new(
            &self.engine,
            State {
                wasi: WasiCtxBuilder::new().build(),
                table: ResourceTable::new(),
                permissions: permissions.clone(),
                fetcher: self.fetcher.clone(),
                limits: wasmtime::StoreLimitsBuilder::new()
                    .memory_size(MAX_MEMORY)
                    .instances(8)
                    .build(),
                http_calls: 0,
            },
        );
        store.limiter(|s| &mut s.limits);
        store.set_fuel(FUEL_PER_CALL)?;
        Ok(store)
    }
}

/// Marker types for the three kinds of plugin.
pub struct Indexer;
pub struct Notification;
pub struct List;
pub struct DownloadClient;
pub struct MediaServer;

/// A loaded plugin. Each call gets a fresh instance: no state leaks between
/// calls and a trapped plugin cannot poison later calls.
pub struct Plugin<K> {
    host: PluginHost,
    component: Component,
    permissions: Permissions,
    _kind: std::marker::PhantomData<K>,
}

impl<K> Plugin<K> {
    fn new(host: PluginHost, component: Component, permissions: Permissions) -> Self {
        Self {
            host,
            component,
            permissions,
            _kind: std::marker::PhantomData,
        }
    }
}

impl Plugin<Indexer> {
    async fn instantiate(&self) -> Result<(Store<State>, IndexerWorld)> {
        let mut store = self.host.store(&self.permissions)?;
        let world =
            IndexerWorld::instantiate_async(&mut store, &self.component, &self.host.linker).await?;
        Ok((store, world))
    }

    pub async fn settings_schema(&self) -> Result<String> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_indexer()
            .call_settings_schema(&mut store)
            .await
            .map_err(trap)
    }

    pub async fn search(&self, query: &SearchQuery, settings_json: &str) -> Result<Vec<Release>> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_indexer()
            .call_search(&mut store, query, settings_json)
            .await
            .map_err(trap)?
            .map_err(|e| anyhow!("plugin error: {e}"))
    }

    pub async fn recent(&self, settings_json: &str) -> Result<Vec<Release>> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_indexer()
            .call_recent(&mut store, settings_json)
            .await
            .map_err(trap)?
            .map_err(|e| anyhow!("plugin error: {e}"))
    }

    pub async fn test(&self, settings_json: &str) -> Result<()> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_indexer()
            .call_test(&mut store, settings_json)
            .await
            .map_err(trap)?
            .map_err(|e| anyhow!("{e}"))
    }
}

impl Plugin<Notification> {
    async fn instantiate(&self) -> Result<(Store<State>, NotificationWorld)> {
        let mut store = self.host.store(&self.permissions)?;
        let world =
            NotificationWorld::instantiate_async(&mut store, &self.component, &self.host.linker)
                .await?;
        Ok((store, world))
    }

    pub async fn settings_schema(&self) -> Result<String> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_notification()
            .call_settings_schema(&mut store)
            .await
            .map_err(trap)
    }

    pub async fn send(&self, event: &Event, settings_json: &str) -> Result<()> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_notification()
            .call_send(&mut store, event, settings_json)
            .await
            .map_err(trap)?
            .map_err(|e| anyhow!("{e}"))
    }
}

impl Plugin<List> {
    async fn instantiate(&self) -> Result<(Store<State>, ListWorld)> {
        let mut store = self.host.store(&self.permissions)?;
        let world =
            ListWorld::instantiate_async(&mut store, &self.component, &self.host.linker).await?;
        Ok((store, world))
    }

    pub async fn settings_schema(&self) -> Result<String> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_import_list()
            .call_settings_schema(&mut store)
            .await
            .map_err(trap)
    }

    pub async fn fetch(&self, settings_json: &str) -> Result<Vec<ListItem>> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_import_list()
            .call_fetch(&mut store, settings_json)
            .await
            .map_err(trap)?
            .map_err(|e| anyhow!("{e}"))
    }
}

impl Plugin<DownloadClient> {
    async fn instantiate(&self) -> Result<(Store<State>, DownloadClientWorld)> {
        let mut store = self.host.store(&self.permissions)?;
        let world =
            DownloadClientWorld::instantiate_async(&mut store, &self.component, &self.host.linker)
                .await?;
        Ok((store, world))
    }

    pub async fn settings_schema(&self) -> Result<String> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_download_client()
            .call_settings_schema(&mut store)
            .await
            .map_err(trap)
    }

    pub async fn add(&self, request: &ClientAddRequest, settings_json: &str) -> Result<String> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_download_client()
            .call_add(&mut store, request, settings_json)
            .await
            .map_err(trap)?
            .map_err(|e| anyhow!("{e}"))
    }

    pub async fn items(&self, settings_json: &str) -> Result<Vec<ClientItemInfo>> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_download_client()
            .call_items(&mut store, settings_json)
            .await
            .map_err(trap)?
            .map_err(|e| anyhow!("{e}"))
    }

    pub async fn remove(&self, id: &str, delete_data: bool, settings_json: &str) -> Result<()> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_download_client()
            .call_remove(&mut store, id, delete_data, settings_json)
            .await
            .map_err(trap)?
            .map_err(|e| anyhow!("{e}"))
    }

    pub async fn pause(&self, id: &str, settings_json: &str) -> Result<()> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_download_client()
            .call_pause(&mut store, id, settings_json)
            .await
            .map_err(trap)?
            .map_err(|e| anyhow!("{e}"))
    }

    pub async fn resume(&self, id: &str, settings_json: &str) -> Result<()> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_download_client()
            .call_resume(&mut store, id, settings_json)
            .await
            .map_err(trap)?
            .map_err(|e| anyhow!("{e}"))
    }

    pub async fn test(&self, settings_json: &str) -> Result<()> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_download_client()
            .call_test(&mut store, settings_json)
            .await
            .map_err(trap)?
            .map_err(|e| anyhow!("{e}"))
    }
}

impl Plugin<MediaServer> {
    async fn instantiate(&self) -> Result<(Store<State>, MediaServerWorld)> {
        let mut store = self.host.store(&self.permissions)?;
        let world =
            MediaServerWorld::instantiate_async(&mut store, &self.component, &self.host.linker)
                .await?;
        Ok((store, world))
    }

    pub async fn settings_schema(&self) -> Result<String> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_media_server()
            .call_settings_schema(&mut store)
            .await
            .map_err(trap)
    }

    /// One page of the server's library; `None` asks for the first.
    pub async fn library_page(
        &self,
        cursor: Option<&str>,
        settings_json: &str,
    ) -> Result<MediaPage> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_media_server()
            .call_library_page(&mut store, cursor, settings_json)
            .await
            .map_err(trap)?
            .map_err(|e| anyhow!("{e}"))
    }

    /// The server's name, if the settings work.
    pub async fn test(&self, settings_json: &str) -> Result<String> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_media_server()
            .call_test(&mut store, settings_json)
            .await
            .map_err(trap)?
            .map_err(|e| anyhow!("{e}"))
    }
}

/// A trap or host error from wasmtime, as the error type the rest of the app uses.
fn trap(e: wasmtime::Error) -> anyhow::Error {
    anyhow!("plugin trapped: {e:#}")
}

/// Any other wasmtime error, without the "trapped" wording.
fn wt(e: wasmtime::Error) -> anyhow::Error {
    anyhow!("{e:#}")
}
