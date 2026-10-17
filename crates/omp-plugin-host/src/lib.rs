//! WASM plugin runtime.
//!
//! Plugins are WASI components (see `wit/omp-plugin.wit`). They get no ambient
//! network or filesystem access: the only way out is the `host` interface,
//! which checks every call against the plugin's declared permissions.

use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, anyhow};
use wasmtime::{
    Config, Engine, Store,
    component::{Component, Linker, ResourceTable},
};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "indexer-plugin",
        imports: { default: async },
        exports: { default: async },
    });
}

use bindings::{IndexerPlugin, exports::omp::plugin::indexer as ix, omp::plugin::host};

pub use ix::{Release, SearchQuery};

/// Does the actual HTTP work for plugins. The host owns this so it can add
/// rate limiting, caching and proxy settings in one place.
#[async_trait::async_trait]
pub trait Fetcher: Send + Sync {
    async fn get(&self, url: &str) -> Result<String>;
}

/// What a plugin declared it needs (from `plugin.toml`).
#[derive(Debug, Clone, Default)]
pub struct Permissions {
    /// Exact hostnames the plugin may call via `http-get`.
    pub allowed_hosts: Vec<String>,
}

/// Fuel given to each call. Bounds CPU use so a looping plugin cannot hang the host.
const FUEL_PER_CALL: u64 = 500_000_000;

pub struct PluginHost {
    engine: Engine,
    linker: Linker<State>,
    fetcher: Arc<dyn Fetcher>,
}

pub struct Plugin {
    host: PluginHost,
    component: Component,
    permissions: Permissions,
}

struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    permissions: Permissions,
    fetcher: Arc<dyn Fetcher>,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
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

    async fn http_get(&mut self, url: String) -> Result<String, String> {
        let parsed = url::Url::parse(&url).map_err(|e| format!("invalid url: {e}"))?;
        let host = parsed.host_str().unwrap_or_default();
        if !self.permissions.allowed_hosts.iter().any(|h| h == host) {
            return Err(format!("host {host:?} is not allowed for this plugin"));
        }
        self.fetcher.get(&url).await.map_err(|e| e.to_string())
    }
}

impl PluginHost {
    pub fn new(fetcher: Arc<dyn Fetcher>) -> Result<Self> {
        let mut config = Config::new();
        config.async_support(true);
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

    pub fn load(&self, wasm: impl AsRef<Path>, permissions: Permissions) -> Result<Plugin> {
        let path = wasm.as_ref();
        let component = Component::from_file(&self.engine, path)
            .with_context(|| format!("loading plugin {}", path.display()))?;
        Ok(Plugin {
            host: PluginHost {
                engine: self.engine.clone(),
                linker: self.linker.clone(),
                fetcher: self.fetcher.clone(),
            },
            component,
            permissions,
        })
    }
}

impl Plugin {
    /// Each call gets a fresh instance: no state leaks between calls and a
    /// trapped plugin cannot poison later calls.
    async fn instantiate(&self) -> Result<(Store<State>, IndexerPlugin)> {
        let mut store = Store::new(
            &self.host.engine,
            State {
                wasi: WasiCtxBuilder::new().build(),
                table: ResourceTable::new(),
                permissions: self.permissions.clone(),
                fetcher: self.host.fetcher.clone(),
            },
        );
        store.set_fuel(FUEL_PER_CALL)?;
        let plugin =
            IndexerPlugin::instantiate_async(&mut store, &self.component, &self.host.linker)
                .await?;
        Ok((store, plugin))
    }

    pub async fn settings_schema(&self) -> Result<String> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_indexer()
            .call_settings_schema(&mut store)
            .await
    }

    pub async fn search(&self, query: &SearchQuery, settings_json: &str) -> Result<Vec<Release>> {
        let (mut store, plugin) = self.instantiate().await?;
        plugin
            .omp_plugin_indexer()
            .call_search(&mut store, query, settings_json)
            .await
            .context("plugin trapped")?
            .map_err(|e| anyhow!("plugin error: {e}"))
    }
}
