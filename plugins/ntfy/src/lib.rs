#![allow(dead_code)]

wit_bindgen::generate!({
    path: "../../wit",
    world: "notification-plugin",
});

use exports::omp::plugin::notification::{Event, Guest};
use omp::plugin::host::{self, HttpRequest};
use serde::Deserialize;
use serde_json::json;

fn ok(status: u16) -> bool {
    (200..300).contains(&status)
}

fn post(url: String, headers: Vec<(String, String)>, body: String) -> Result<(), String> {
    let response = host::http(&HttpRequest { method: "POST".into(), url, headers, body: Some(body) })?;
    if ok(response.status) { Ok(()) } else { Err(format!("the server answered {}: {}", response.status, response.body.chars().take(200).collect::<String>())) }
}

fn get(url: String, headers: Vec<(String, String)>) -> Result<(), String> {
    let response = host::http(&HttpRequest { method: "GET".into(), url, headers, body: None })?;
    if ok(response.status) { Ok(()) } else { Err(format!("the server answered {}", response.status)) }
}

fn json_header() -> Vec<(String, String)> {
    vec![("content-type".to_owned(), "application/json".to_owned())]
}

fn text(event: &Event) -> String {
    if event.message.is_empty() { event.title.clone() } else { format!("{}\n{}", event.title, event.message) }
}

fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[derive(Deserialize)]
struct Settings {
    #[serde(default)]
    server: Option<String>,
    topic: String,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    priority: Option<u8>,
}

struct Plugin;

impl Guest for Plugin {
    fn settings_schema() -> String {
        json!({
            "type": "object",
            "required": ["topic"],
            "properties": {
                "server": { "type": "string", "title": "Server", "default": "https://ntfy.sh", "format": "uri" },
                "topic": { "type": "string", "title": "Topic" },
                "access_token": { "type": "string", "title": "Access token", "format": "password" },
                "priority": { "type": "integer", "title": "Priority (1 to 5)", "default": 3 }
            }
        })
        .to_string()
    }

    fn send(event: Event, settings_json: String) -> Result<(), String> {
        let s: Settings = serde_json::from_str(&settings_json).map_err(|e| format!("bad settings: {e}"))?;
        if s.topic.trim().is_empty() {
            return Err("the topic is empty".into());
        }
        let server = s.server.filter(|x| !x.trim().is_empty()).unwrap_or_else(|| "https://ntfy.sh".into());
        let mut headers = vec![
            ("title".to_owned(), event.title.clone()),
            ("priority".to_owned(), s.priority.unwrap_or(3).clamp(1, 5).to_string()),
            ("tags".to_owned(), event.kind.clone()),
        ];
        if let Some(t) = s.access_token.filter(|t| !t.is_empty()) {
            headers.push(("authorization".into(), format!("Bearer {t}")));
        }
        post(format!("{}/{}", server.trim_end_matches('/'), enc(s.topic.trim())), headers, event.message)
    }
}

export!(Plugin);
