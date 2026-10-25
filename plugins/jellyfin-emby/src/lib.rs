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
    server: String,
    api_key: String,
}

struct Plugin;

impl Guest for Plugin {
    fn settings_schema() -> String {
        json!({
            "type": "object",
            "required": ["server", "api_key"],
            "properties": {
                "server": { "type": "string", "title": "Server URL", "format": "uri", "description": "For example http://192.168.1.10:8096" },
                "api_key": { "type": "string", "title": "API key", "format": "password", "description": "Dashboard, API keys." }
            }
        })
        .to_string()
    }

    fn send(event: Event, settings_json: String) -> Result<(), String> {
        let s: Settings = serde_json::from_str(&settings_json).map_err(|e| format!("bad settings: {e}"))?;
        if s.server.trim().is_empty() || s.api_key.trim().is_empty() {
            return Err("the server and API key are needed".into());
        }
        let base = s.server.trim_end_matches('/');
        let headers = vec![("x-emby-token".to_owned(), s.api_key.clone())];
        if event.kind == "test" {
            return get(format!("{base}/System/Info"), headers);
        }
        post(format!("{base}/Library/Refresh"), headers, String::new())
    }
}

export!(Plugin);
