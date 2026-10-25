//! Settings about how the app itself is reached and how it reaches the web:
//! port, URL base, HTTPS and an outbound proxy. They are read at start-up,
//! so a change needs a restart. The environment wins over what is saved.

use std::path::PathBuf;

use axum::{Json, extract::State};
use omp_db::Db;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    api::{ApiError, ApiResult},
    app::App,
    config::AppConfig,
};

const KEY: &str = "host";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct HostSettings {
    /// Port to listen on; `None` keeps the default (8080).
    pub port: Option<u16>,
    pub url_base: String,
    pub ssl: Ssl,
    pub proxy: ProxySettings,
    pub log: LogSettings,
    /// A TMDB read token of the owner's own; empty uses the built-in one.
    pub tmdb_token: String,
}

/// The event log: how much detail, and how long it is kept.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LogSettings {
    /// `info`, `debug` or `trace`.
    pub level: String,
    pub keep_days: i64,
    pub max_rows: i64,
}

impl Default for LogSettings {
    fn default() -> Self {
        Self {
            level: "info".into(),
            keep_days: 14,
            max_rows: 50_000,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Ssl {
    pub enabled: bool,
    pub cert_path: String,
    pub key_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ProxySettings {
    pub enabled: bool,
    /// `http`, `https` or `socks5`.
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    /// Comma separated hosts that skip the proxy.
    pub bypass: String,
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            scheme: "http".into(),
            host: String::new(),
            port: 8080,
            username: String::new(),
            password: String::new(),
            bypass: "localhost,127.0.0.1".into(),
        }
    }
}

pub async fn load(db: &Db) -> HostSettings {
    match db.config().get(KEY).await {
        Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_default(),
        _ => HostSettings::default(),
    }
}

/// Puts the saved settings into `config` (the environment's choices stay)
/// and hands the proxy to the HTTP clients.
pub async fn apply(db: &Db, config: &mut AppConfig) {
    let saved = load(db).await;
    if !config.addr_pinned
        && let Some(port) = saved.port
    {
        config.addr.set_port(port);
    }
    if !config.url_base_pinned {
        config.url_base = crate::ui_files::normalize_base(&saved.url_base);
    }
    if saved.ssl.enabled {
        config.tls = Some(crate::config::TlsFiles {
            cert: PathBuf::from(&saved.ssl.cert_path),
            key: PathBuf::from(&saved.ssl.key_path),
        });
    }
    // The environment's token wins, then the one saved here, then the built-in one.
    if config.tmdb_token.is_none() {
        config.tmdb_token = Some(
            Some(saved.tmdb_token.trim().to_owned())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| omp_metadata::tmdb::DEFAULT_READ_TOKEN.to_owned()),
        );
    }
    omp_net::set_proxy(proxy(&saved.proxy));
    crate::logs::configure(&saved.log.level, saved.log.keep_days, saved.log.max_rows);
}

fn proxy(p: &ProxySettings) -> Option<omp_net::Proxy> {
    (p.enabled && !p.host.trim().is_empty()).then(|| omp_net::Proxy {
        scheme: p.scheme.clone(),
        host: p.host.trim().to_owned(),
        port: p.port,
        username: Some(p.username.clone()).filter(|u| !u.is_empty()),
        password: Some(p.password.clone()).filter(|u| !u.is_empty()),
        bypass: p
            .bypass
            .split(',')
            .map(|h| h.trim().to_owned())
            .filter(|h| !h.is_empty())
            .collect(),
    })
}

fn validate(s: &HostSettings) -> Result<(), String> {
    if s.port == Some(0) {
        return Err("the port must be between 1 and 65535".into());
    }
    let base = crate::ui_files::normalize_base(&s.url_base);
    if base.contains(char::is_whitespace) || base.contains(['?', '#', '\\']) {
        return Err("the URL base can only hold letters, digits and slashes".into());
    }
    if s.ssl.enabled {
        crate::tls::load_certified_key(&s.ssl.cert_path, &s.ssl.key_path)
            .map_err(|e| format!("HTTPS: {e}"))?;
    }
    let token = s.tmdb_token.trim();
    if !token.is_empty() && !(token.starts_with("eyJ") && token.matches('.').count() == 2) {
        return Err(
            "the TMDB token is the long \"API Read Access Token\" (it starts with eyJ), not the short API key"
                .into(),
        );
    }
    if !matches!(s.log.level.as_str(), "info" | "debug" | "trace") {
        return Err("the log level must be info, debug or trace".into());
    }
    if !(1..=365).contains(&s.log.keep_days) {
        return Err("keep the log for 1 to 365 days".into());
    }
    if !(1_000..=1_000_000).contains(&s.log.max_rows) {
        return Err("keep between 1,000 and 1,000,000 log entries".into());
    }
    if s.proxy.enabled {
        if s.proxy.host.trim().is_empty() {
            return Err("the proxy needs an address".into());
        }
        if s.proxy.port == 0 {
            return Err("the proxy needs a port".into());
        }
        if !matches!(s.proxy.scheme.as_str(), "http" | "https" | "socks5") {
            return Err("the proxy type must be http, https or socks5".into());
        }
    }
    Ok(())
}

fn view(app: &App, saved: &HostSettings) -> Value {
    let c = &app.config;
    json!({
        "port": saved.port,
        "url_base": saved.url_base,
        "ssl": saved.ssl,
        "log": saved.log,
        "proxy": {
            "enabled": saved.proxy.enabled,
            "scheme": saved.proxy.scheme,
            "host": saved.proxy.host,
            "port": saved.proxy.port,
            "username": saved.proxy.username,
            // The password is never sent back; an empty one on save keeps it.
            "has_password": !saved.proxy.password.is_empty(),
            "bypass": saved.proxy.bypass,
        },
        "tmdb": {
            // The token itself is never sent back.
            "custom": !saved.tmdb_token.trim().is_empty(),
            "from_environment": std::env::var("TMDB_READ_TOKEN").is_ok_and(|v| !v.trim().is_empty()),
        },
        // What the running server is actually using.
        "running": {
            "port": c.addr.port(),
            "url_base": c.url_base,
            "ssl": c.tls.is_some(),
            "port_from_environment": c.addr_pinned,
            "url_base_from_environment": c.url_base_pinned,
        },
    })
}

pub async fn get_host(State(app): State<std::sync::Arc<App>>) -> ApiResult {
    let saved = load(&app.db).await;
    Ok(Json(view(&app, &saved)))
}

#[derive(Deserialize)]
pub struct HostBody {
    port: Option<u16>,
    #[serde(default)]
    url_base: String,
    #[serde(default)]
    ssl: Ssl,
    #[serde(default)]
    proxy: ProxyBody,
    #[serde(default)]
    log: LogSettings,
    /// `None` keeps the saved token; an empty one goes back to the built-in token.
    tmdb_token: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ProxyBody {
    enabled: bool,
    scheme: Option<String>,
    host: String,
    port: Option<u16>,
    username: String,
    /// `None` keeps the saved password.
    password: Option<String>,
    bypass: Option<String>,
}

pub async fn put_host(
    State(app): State<std::sync::Arc<App>>,
    Json(body): Json<HostBody>,
) -> ApiResult {
    let old = load(&app.db).await;
    let defaults = ProxySettings::default();
    let new = HostSettings {
        port: body.port,
        url_base: crate::ui_files::normalize_base(&body.url_base),
        ssl: body.ssl,
        log: body.log,
        tmdb_token: body.tmdb_token.unwrap_or(old.tmdb_token),
        proxy: ProxySettings {
            enabled: body.proxy.enabled,
            scheme: body.proxy.scheme.unwrap_or(defaults.scheme),
            host: body.proxy.host.trim().to_owned(),
            port: body.proxy.port.unwrap_or(defaults.port),
            username: body.proxy.username,
            password: body.proxy.password.unwrap_or(old.proxy.password),
            bypass: body.proxy.bypass.unwrap_or(defaults.bypass),
        },
    };
    validate(&new).map_err(ApiError::bad_request)?;
    app.db
        .config()
        .set(
            KEY,
            &serde_json::to_string(&new).map_err(|e| ApiError::internal(e.to_string()))?,
        )
        .await?;
    // The log settings need no restart.
    crate::logs::configure(&new.log.level, new.log.keep_days, new.log.max_rows);
    Ok(Json(view(&app, &new)))
}

// ---- how the UI looks ---------------------------------------------------------------------

const UI_KEY: &str = "ui";

/// What the browser UI shows and how. Kept on the server so every browser
/// agrees.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UiPrefs {
    /// Language code of a translation the UI has (`en`).
    pub language: String,
    /// `locale` (the browser's), `iso` (2025-01-31), `dmy` or `mdy`.
    pub date_format: String,
    /// `locale`, `12h` or `24h`.
    pub time_format: String,
    /// `auto` follows the system.
    pub theme: String,
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            language: "en".into(),
            date_format: "locale".into(),
            time_format: "locale".into(),
            theme: "auto".into(),
        }
    }
}

fn check_prefs(p: &UiPrefs) -> Result<(), String> {
    let one_of = |what: &str, value: &str, allowed: &[&str]| {
        if allowed.contains(&value) {
            Ok(())
        } else {
            Err(format!("{what} must be one of: {}", allowed.join(", ")))
        }
    };
    one_of("language", &p.language, &["en"])?;
    one_of(
        "date_format",
        &p.date_format,
        &["locale", "iso", "dmy", "mdy"],
    )?;
    one_of("time_format", &p.time_format, &["locale", "12h", "24h"])?;
    one_of("theme", &p.theme, &["auto", "light", "dark"])
}

pub async fn get_ui(State(app): State<std::sync::Arc<App>>) -> ApiResult {
    let prefs: UiPrefs = match app.db.config().get(UI_KEY).await? {
        Some(json) => serde_json::from_str(&json).unwrap_or_default(),
        None => UiPrefs::default(),
    };
    Ok(Json(json!(prefs)))
}

pub async fn put_ui(
    State(app): State<std::sync::Arc<App>>,
    Json(prefs): Json<UiPrefs>,
) -> ApiResult {
    check_prefs(&prefs).map_err(ApiError::bad_request)?;
    app.db
        .config()
        .set(
            UI_KEY,
            &serde_json::to_string(&prefs).map_err(|e| ApiError::internal(e.to_string()))?,
        )
        .await?;
    Ok(Json(json!(prefs)))
}
