//! Tells Plex, Jellyfin or Emby to rescan its library after an import or upgrade.
//! Does nothing for other event kinds, since a scan is only useful when a file changed.
//!
//! Plex: <https://support.plex.tv/articles/201638786-plex-media-server-url-commands/>
//! Jellyfin/Emby: both expose the same Emby-derived `Library/Refresh` endpoint.

wit_bindgen::generate!({
    path: "../../wit",
    world: "notification-plugin",
});

use exports::omp::plugin::notification::{Event, Guest};
use omp::plugin::host::{self, HttpRequest};
use serde::Deserialize;

#[derive(Deserialize)]
struct Settings {
    server_type: String,
    server: String,
    #[serde(default)]
    plex_token: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
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

        match s.server_type.as_str() {
            "plex" => scan_plex(server, s.plex_token.as_deref()),
            "jellyfin" | "emby" => scan_emby_like(server, s.api_key.as_deref()),
            other => Err(format!("unknown server type: {other}")),
        }
    }
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
