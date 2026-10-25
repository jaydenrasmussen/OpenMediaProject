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
    webhook_url: String,
    #[serde(default)]
    username: Option<String>,
}

struct Plugin;

impl Guest for Plugin {
    fn settings_schema() -> String {
        json!({
            "type": "object",
            "required": ["webhook_url"],
            "properties": {
                "webhook_url": { "type": "string", "title": "Webhook URL", "format": "uri", "description": "Channel settings, Integrations, Webhooks." },
                "username": { "type": "string", "title": "Name to post as", "default": "OpenMediaProject" }
            }
        })
        .to_string()
    }

    fn send(event: Event, settings_json: String) -> Result<(), String> {
        let s: Settings = serde_json::from_str(&settings_json).map_err(|e| format!("bad settings: {e}"))?;
        if !s.webhook_url.starts_with("https://") {
            return Err("the webhook URL must start with https://".into());
        }
        let color = match event.kind.as_str() {
            "download-failed" | "health-issue" => 0xE5_56_6F,
            "import" => 0x3F_C0_7A,
            _ => 0x6C_8C_FF,
        };
        let body = json!({
            "username": s.username.unwrap_or_else(|| "OpenMediaProject".into()),
            "embeds": [{ "title": event.title, "description": event.message, "color": color }],
        })
        .to_string();
        post(s.webhook_url, json_header(), body)
    }
}

export!(Plugin);
