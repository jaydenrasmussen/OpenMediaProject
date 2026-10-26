//! Tells Plex, Jellyfin or Emby to rescan its library after an import or upgrade.
//! Does nothing for other event kinds, since a scan is only useful when a file changed.
//!
//! A season pack or a batch of movies imports as several history rows close
//! together, each of which fires its own notification event. Without
//! debouncing, that means one full-library rescan request per file, which
//! can be expensive on a large library. Storage remembers the last time a
//! scan was actually requested (per server, so several configured servers
//! do not share a cooldown) and skips the call if the minimum interval has
//! not elapsed yet; the next import after the window re-triggers it, so
//! nothing is permanently missed.
//!
//! Plex: <https://support.plex.tv/articles/201638786-plex-media-server-url-commands/>
//! Jellyfin/Emby: both expose the same Emby-derived `Library/Refresh` endpoint.

wit_bindgen::generate!({
    path: "../../wit",
    world: "notification-plugin",
});

use std::time::{SystemTime, UNIX_EPOCH};

use exports::omp::plugin::notification::{Event, Guest};
use omp::plugin::host::{self, HttpRequest};
use serde::Deserialize;

/// Skip a rescan request if the last one for this server was within this
/// many seconds, unless the caller's settings say otherwise.
const DEFAULT_MIN_INTERVAL_SECONDS: u64 = 60;

#[derive(Deserialize)]
struct Settings {
    server_type: String,
    server: String,
    #[serde(default)]
    plex_token: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    /// Minimum seconds between scan requests to this server. 0 disables the
    /// debounce entirely (always scan).
    #[serde(default = "default_min_interval")]
    min_interval_seconds: u64,
}

fn default_min_interval() -> u64 {
    DEFAULT_MIN_INTERVAL_SECONDS
}

struct MediaServerScan;

impl Guest for MediaServerScan {
    fn settings_schema() -> String {
        r#"{
            "type": "object",
            "required": ["server_type", "server"],
            "properties": {
                "server_type": {
                    "type": "string",
                    "title": "Server type",
                    "enum": ["plex", "jellyfin", "emby"],
                    "default": "plex"
                },
                "server": {
                    "type": "string",
                    "title": "Server URL",
                    "format": "uri",
                    "description": "e.g. http://192.168.1.10:32400 (Plex) or http://192.168.1.10:8096 (Jellyfin/Emby)."
                },
                "plex_token": {
                    "type": "string",
                    "title": "Plex token",
                    "format": "password",
                    "description": "Only used when server type is Plex."
                },
                "api_key": {
                    "type": "string",
                    "title": "API key",
                    "format": "password",
                    "description": "Only used when server type is Jellyfin or Emby."
                },
                "min_interval_seconds": {
                    "type": "integer",
                    "title": "Minimum seconds between scans",
                    "description": "A batch of imports (e.g. a season pack) only triggers one scan per this many seconds. 0 scans every time.",
                    "default": 60,
                    "minimum": 0
                }
            }
        }"#
        .to_owned()
    }

    fn send(event: Event, settings_json: String) -> Result<(), String> {
        // A scan is only useful after a file on disk actually changed.
        if !matches!(event.kind.as_str(), "import" | "upgrade" | "test") {
            return Ok(());
        }

        let s: Settings =
            serde_json::from_str(&settings_json).map_err(|e| format!("bad settings: {e}"))?;
        let server = s.server.trim().trim_end_matches('/');
        if server.is_empty() {
            return Err("the server URL is empty".into());
        }

        // "test" always scans, even inside the cooldown, so the Test button
        // in settings gives an honest answer.
        let storage_key = format!("last_scan:{server}");
        if event.kind != "test"
            && s.min_interval_seconds > 0
            && !cooldown_elapsed(&storage_key, s.min_interval_seconds)
        {
            return Ok(());
        }

        let result = match s.server_type.as_str() {
            "plex" => scan_plex(server, s.plex_token.as_deref()),
            "jellyfin" | "emby" => scan_emby_like(server, s.api_key.as_deref()),
            other => Err(format!("unknown server type: {other}")),
        };
        if result.is_ok() {
            host::storage_set(&storage_key, &now_unix_seconds().to_string());
        }
        result
    }
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Whether enough time passed since the last scan recorded under `key` that
/// a new one is allowed. True (allowed) if nothing was recorded yet, if the
/// stored value is unreadable, or if it is already in the future (the clock
/// moved back): fail open, since skipping a real library change is worse
/// than one extra scan.
fn cooldown_elapsed(key: &str, min_interval_seconds: u64) -> bool {
    let Some(last) = host::storage_get(key).and_then(|v| v.parse::<u64>().ok()) else {
        return true;
    };
    let now = now_unix_seconds();
    now < last || now - last >= min_interval_seconds
}

fn scan_plex(server: &str, token: Option<&str>) -> Result<(), String> {
    let token = token
        .filter(|t| !t.is_empty())
        .ok_or("the Plex token is empty")?;
    let response = host::http(&HttpRequest {
        method: "GET".to_owned(),
        url: format!("{server}/library/sections/all/refresh?X-Plex-Token={token}"),
        headers: Vec::new(),
        body: None,
    })?;
    if (200..300).contains(&response.status) {
        Ok(())
    } else {
        Err(format!(
            "Plex answered {}: {}",
            response.status, response.body
        ))
    }
}

fn scan_emby_like(server: &str, api_key: Option<&str>) -> Result<(), String> {
    let api_key = api_key
        .filter(|k| !k.is_empty())
        .ok_or("the API key is empty")?;
    let response = host::http(&HttpRequest {
        method: "POST".to_owned(),
        url: format!("{server}/Library/Refresh"),
        headers: vec![("x-emby-token".to_owned(), api_key.to_owned())],
        body: None,
    })?;
    if (200..300).contains(&response.status) {
        Ok(())
    } else {
        Err(format!(
            "the server answered {}: {}",
            response.status, response.body
        ))
    }
}

export!(MediaServerScan);
