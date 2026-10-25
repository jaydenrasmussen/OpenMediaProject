//! Installing and removing plugins at run time.
//!
//! A plugin package is a zip file holding `plugin.toml` and the `.wasm` it
//! names (at the top, or inside one folder). Installing is two steps so the
//! person sees what they are agreeing to: a preview that downloads and checks
//! the package and reports what it may do (the hosts it may reach) with the
//! package's SHA-256, then an install that must quote that hash, so what is
//! installed is exactly what was shown.

use std::{
    io::{Cursor, Read},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, post},
};
use omp_plugin_host::Manifest;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    api::{ApiError, ApiResult, Shared},
    app::App,
    plugins::Plugins,
};

const MAX_PACKAGE: usize = 32 * 1024 * 1024;
const MAX_FILE: u64 = 64 * 1024 * 1024;

pub(crate) fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/plugin/install", post(install))
        .route("/plugin/{id}", delete(uninstall))
        .route("/plugin/sources", axum::routing::get(sources))
}

/// Where each plugin the user installed came from. It lives in the main
/// database, so a backup carries it and a restored install can fetch the
/// plugins again.
const SOURCES_KEY: &str = "plugin_sources";

async fn load_sources(app: &App) -> Result<serde_json::Map<String, Value>, ApiError> {
    Ok(match app.db.config().get(SOURCES_KEY).await? {
        Some(json) => serde_json::from_str(&json).unwrap_or_default(),
        None => Default::default(),
    })
}

async fn save_sources(app: &App, map: &serde_json::Map<String, Value>) -> Result<(), ApiError> {
    app.db
        .config()
        .set(SOURCES_KEY, &Value::Object(map.clone()).to_string())
        .await?;
    Ok(())
}

/// The plugins installed from a link, and whether each is installed now.
async fn sources(State(app): Shared) -> ApiResult {
    let present: Vec<String> = app
        .plugins()
        .manifests()
        .iter()
        .map(|m| m.id.clone())
        .collect();
    let list: Vec<Value> = load_sources(&app)
        .await?
        .into_iter()
        .map(|(id, mut v)| {
            v["id"] = json!(id);
            v["installed"] = json!(present.contains(&id));
            v
        })
        .collect();
    Ok(Json(Value::Array(list)))
}

/// Where plugins installed by the user live: the last plugin folder, which
/// also wins over bundled copies of the same plugin.
pub(crate) fn user_dir(app: &App) -> PathBuf {
    app.config
        .plugin_dirs
        .last()
        .cloned()
        .unwrap_or_else(|| app.config.data_dir.join("plugins"))
}

struct Package {
    manifest: Manifest,
    manifest_text: String,
    wasm: Vec<u8>,
    sha256: String,
}

async fn download(url: &str) -> Result<Vec<u8>, ApiError> {
    let parsed = url::Url::parse(url).map_err(|_| ApiError::bad_request("that isn't a URL"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ApiError::bad_request("the URL must be http or https"));
    }
    let client = omp_net::client_builder()
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut response = client.get(parsed).send().await.map_err(|e| {
        ApiError(
            StatusCode::BAD_GATEWAY,
            format!("download failed: {}", e.without_url()),
        )
    })?;
    if !response.status().is_success() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            format!("the server answered {}", response.status()),
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| {
        ApiError(
            StatusCode::BAD_GATEWAY,
            format!("download failed: {}", e.without_url()),
        )
    })? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_PACKAGE {
            return Err(ApiError::bad_request("the package is larger than 32 MB"));
        }
    }
    Ok(bytes)
}

fn read_entry(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, index: usize) -> Result<Vec<u8>, ApiError> {
    let file = zip
        .by_index(index)
        .map_err(|e| ApiError::bad_request(format!("unreadable package: {e}")))?;
    if file.size() > MAX_FILE {
        return Err(ApiError::bad_request("a file in the package is too large"));
    }
    let mut out = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut out)
        .map_err(|e| ApiError::bad_request(format!("unreadable package: {e}")))?;
    Ok(out)
}

fn open(bytes: &[u8]) -> Result<Package, ApiError> {
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| ApiError::bad_request("that isn't a zip file"))?;
    // Names are looked at, never used as paths.
    let mut names: Vec<(usize, String)> = Vec::new();
    for i in 0..zip.len() {
        let f = zip
            .by_index(i)
            .map_err(|e| ApiError::bad_request(format!("unreadable package: {e}")))?;
        if f.is_dir() {
            continue;
        }
        // A name the zip library cannot read safely is left out.
        if let Ok(name) = f.name() {
            names.push((i, name.replace('\\', "/")));
        }
    }
    let manifest_at = names
        .iter()
        .find(|(_, n)| {
            n == "plugin.toml" || (n.ends_with("/plugin.toml") && n.matches('/').count() == 1)
        })
        .cloned()
        .ok_or_else(|| ApiError::bad_request("the package has no plugin.toml"))?;
    let prefix = manifest_at.1.trim_end_matches("plugin.toml").to_owned();
    let manifest_text = String::from_utf8(read_entry(&mut zip, manifest_at.0)?)
        .map_err(|_| ApiError::bad_request("plugin.toml isn't text"))?;
    let manifest = Manifest::parse(&manifest_text, std::path::Path::new(""))
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    let wasm_name = format!("{prefix}{}", manifest.wasm);
    let wasm_at = names
        .iter()
        .find(|(_, n)| *n == wasm_name)
        .ok_or_else(|| ApiError::bad_request(format!("the package has no {}", manifest.wasm)))?;
    let wasm = read_entry(&mut zip, wasm_at.0)?;
    if !wasm.starts_with(b"\0asm") {
        return Err(ApiError::bad_request("the plugin file isn't WebAssembly"));
    }
    Ok(Package {
        manifest,
        manifest_text,
        wasm,
        sha256,
    })
}

/// Does the package load? Tries it in a scratch folder with the real runtime.
async fn check_loads(p: &Package) -> Result<(), ApiError> {
    let scratch = tempfile::tempdir().map_err(|e| ApiError::internal(e.to_string()))?;
    let dir = scratch.path().join(&p.manifest.id);
    std::fs::create_dir_all(&dir).map_err(|e| ApiError::internal(e.to_string()))?;
    std::fs::write(dir.join("plugin.toml"), &p.manifest_text)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    std::fs::write(dir.join(&p.manifest.wasm), &p.wasm)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let root = scratch.path().to_path_buf();
    let id = p.manifest.id.clone();
    let loaded = tokio::task::spawn_blocking(move || Plugins::load(&[root]))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if loaded.manifests().iter().any(|m| m.id == id) {
        Ok(())
    } else {
        let why = loaded
            .problems()
            .first()
            .cloned()
            .unwrap_or_else(|| "it did not load".into());
        Err(ApiError::bad_request(format!(
            "this plugin does not work with this version: {why}"
        )))
    }
}

#[derive(Deserialize)]
struct InstallRequest {
    url: String,
    /// The `sha256` the preview reported. Without it nothing is installed.
    sha256: Option<String>,
}

async fn install(
    State(app): Shared,
    Json(req): Json<InstallRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let bytes = download(req.url.trim()).await?;
    let package = open(&bytes)?;
    check_loads(&package).await?;
    let m = &package.manifest;
    let installed = app
        .plugins()
        .manifests()
        .iter()
        .find(|x| x.id == m.id)
        .map(|x| x.version.clone());
    let summary = json!({
        "id": m.id,
        "name": m.name,
        "version": m.version,
        "kind": format!("{:?}", m.kind).to_lowercase(),
        "description": m.description,
        "allowed_hosts": m.allowed_hosts,
        "sha256": package.sha256,
        "installed_version": installed,
    });
    let Some(approved) = req.sha256 else {
        return Ok((
            StatusCode::OK,
            Json(json!({ "installed": false, "plugin": summary })),
        ));
    };
    if !approved.eq_ignore_ascii_case(&package.sha256) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "the package changed since the preview; look at it again".into(),
        ));
    }
    let root = user_dir(&app);
    let target = root.join(&m.id);
    let staging = root.join(format!(".{}.installing", m.id));
    let write = || -> std::io::Result<()> {
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;
        std::fs::write(staging.join("plugin.toml"), &package.manifest_text)?;
        std::fs::write(staging.join(&m.wasm), &package.wasm)?;
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&staging, &target)
    };
    write().map_err(|e| ApiError::internal(format!("could not write the plugin: {e}")))?;
    app.reload_plugins().await;
    let mut recorded = load_sources(&app).await?;
    recorded.insert(
        m.id.clone(),
        json!({
            "name": m.name,
            "version": m.version,
            "url": req.url.trim(),
            "sha256": package.sha256,
        }),
    );
    save_sources(&app, &recorded).await?;
    tracing::info!(plugin = %m.id, version = %m.version, "plugin installed");
    Ok((
        StatusCode::CREATED,
        Json(json!({ "installed": true, "plugin": summary })),
    ))
}

async fn uninstall(State(app): Shared, Path(id): Path<String>) -> ApiResult<StatusCode> {
    // The id becomes a folder name; it must be a plain one.
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ApiError::bad_request("not a plugin id"));
    }
    let target = user_dir(&app).join(&id);
    if !target.join("plugin.toml").is_file() {
        let bundled = app.plugins().manifests().iter().any(|m| m.id == id);
        return Err(if bundled {
            ApiError::bad_request("that plugin comes with the app and cannot be removed")
        } else {
            ApiError::not_found("no such plugin")
        });
    }
    std::fs::remove_dir_all(&target)
        .map_err(|e| ApiError::internal(format!("could not remove it: {e}")))?;
    app.reload_plugins().await;
    let mut recorded = load_sources(&app).await?;
    if recorded.remove(&id).is_some() {
        save_sources(&app, &recorded).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
