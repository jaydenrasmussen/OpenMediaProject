//! Sends every event as a push notification through a self-hosted Gotify server.
//! API: <https://gotify.net/docs/pushmsg>

wit_bindgen::generate!({
    path: "../../wit",
    world: "notification-plugin",
});

use exports::omp::plugin::notification::{Event, Guest};
use omp::plugin::host::{self, HttpRequest};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct Settings {
    server: String,
    token: String,
    #[serde(default)]
    priority: Option<i64>,
}

struct Gotify;

impl Guest for Gotify {
    fn settings_schema() -> String {
        r#"{
            "type": "object",
            "required": ["server", "token"],
            "properties": {
                "server": { "type": "string", "title": "Server URL", "format": "uri" },
                "token": { "type": "string", "title": "Application token", "format": "password" },
                "priority": { "type": "integer", "title": "Priority", "minimum": 0, "maximum": 10, "default": 5 }
            }
        }"#
        .to_owned()
    }

    fn send(event: Event, settings_json: String) -> Result<(), String> {
        let s: Settings =
            serde_json::from_str(&settings_json).map_err(|e| format!("bad settings: {e}"))?;
        let server = s.server.trim().trim_end_matches('/');
        if server.is_empty() {
            return Err("the server URL is empty".into());
        }
        if s.token.trim().is_empty() {
            return Err("the application token is empty".into());
        }

        let body = json!({
            "title": event.title,
            "message": event.message,
            "priority": priority_for(&event.kind, s.priority),
        })
        .to_string();

        let response = host::http(&HttpRequest {
            method: "POST".to_owned(),
            url: format!("{server}/message?token={}", s.token.trim()),
            headers: vec![("content-type".to_owned(), "application/json".to_owned())],
            body: Some(body),
        })?;
        if (200..300).contains(&response.status) {
            Ok(())
        } else {
            Err(format!(
                "Gotify answered {}: {}",
                response.status, response.body
            ))
        }
    }
}

/// Health issues outrank the user's default priority; everything else uses it as-is.
fn priority_for(kind: &str, default_priority: Option<i64>) -> i64 {
    match kind {
        "health-issue" => default_priority.unwrap_or(5).max(8),
        _ => default_priority.unwrap_or(5),
    }
}

export!(Gotify);
