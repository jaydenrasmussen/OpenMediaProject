//! Sends every event as a push notification through an ntfy server (ntfy.sh or self-hosted).
//! API: <https://docs.ntfy.sh/publish/>

wit_bindgen::generate!({
    path: "../../wit",
    world: "notification-plugin",
});

use exports::omp::plugin::notification::{Event, Guest};
use omp::plugin::host::{self, HttpRequest};
use serde::Deserialize;

#[derive(Deserialize)]
struct Settings {
    server: String,
    topic: String,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    priority: Option<String>,
}

struct Ntfy;

impl Guest for Ntfy {
    fn settings_schema() -> String {
        r#"{
            "type": "object",
            "required": ["server", "topic"],
            "properties": {
                "server": { "type": "string", "title": "Server URL", "format": "uri", "default": "https://ntfy.sh" },
                "topic": { "type": "string", "title": "Topic" },
                "token": { "type": "string", "title": "Access token", "format": "password" },
                "priority": {
                    "type": "string",
                    "title": "Priority",
                    "enum": ["min", "low", "default", "high", "max"],
                    "default": "default"
                }
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
        let topic = s.topic.trim();
        if topic.is_empty() {
            return Err("the topic is empty".into());
        }

        let mut headers = vec![
            ("title".to_owned(), ascii_safe(&event.title)),
            ("tags".to_owned(), tags_for(&event.kind)),
        ];
        if let Some(priority) = s.priority.as_deref().filter(|p| !p.is_empty()) {
            headers.push(("priority".to_owned(), priority.to_owned()));
        }
        if let Some(token) = s.token.as_deref().filter(|t| !t.is_empty()) {
            headers.push(("authorization".to_owned(), format!("Bearer {token}")));
        }

        let response = host::http(&HttpRequest {
            method: "POST".to_owned(),
            url: format!("{server}/{topic}"),
            headers,
            body: Some(event.message),
        })?;
        if (200..300).contains(&response.status) {
            Ok(())
        } else {
            Err(format!(
                "ntfy answered {}: {}",
                response.status, response.body
            ))
        }
    }
}

/// ntfy headers must be ASCII (they go over plain HTTP headers); replace anything else
/// with `?` rather than fail the whole notification over a title's emoji or accent.
fn ascii_safe(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii() && c != '\n' && c != '\r' {
                c
            } else {
                '?'
            }
        })
        .collect()
}

fn tags_for(kind: &str) -> String {
    match kind {
        "grab" => "arrow_down",
        "import" => "white_check_mark",
        "upgrade" => "arrow_up",
        "download-failed" => "x",
        "health-issue" => "warning",
        "health-restored" => "white_check_mark",
        _ => "bell",
    }
    .to_owned()
}

export!(Ntfy);
