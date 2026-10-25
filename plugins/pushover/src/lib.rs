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
    user_key: String,
    api_token: String,
    #[serde(default)]
    priority: Option<i8>,
}

struct Plugin;

impl Guest for Plugin {
    fn settings_schema() -> String {
        json!({
            "type": "object",
            "required": ["user_key", "api_token"],
            "properties": {
                "user_key": { "type": "string", "title": "User key", "format": "password" },
                "api_token": { "type": "string", "title": "Application token", "format": "password" },
                "priority": { "type": "integer", "title": "Priority (-2 to 1)", "default": 0 }
            }
        })
        .to_string()
    }

    fn send(event: Event, settings_json: String) -> Result<(), String> {
        let s: Settings = serde_json::from_str(&settings_json).map_err(|e| format!("bad settings: {e}"))?;
        if s.user_key.trim().is_empty() || s.api_token.trim().is_empty() {
            return Err("the user key and application token are needed".into());
        }
        let form = format!(
            "token={}&user={}&title={}&message={}&priority={}",
            enc(&s.api_token), enc(&s.user_key), enc(&event.title),
            enc(&if event.message.is_empty() { event.title.clone() } else { event.message }),
            s.priority.unwrap_or(0).clamp(-2, 1)
        );
        post("https://api.pushover.net/1/messages.json".into(), vec![("content-type".into(), "application/x-www-form-urlencoded".into())], form)
    }
}

export!(Plugin);
