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
    bot_token: String,
    chat_id: String,
    #[serde(default)]
    silent: bool,
}

struct Plugin;

impl Guest for Plugin {
    fn settings_schema() -> String {
        json!({
            "type": "object",
            "required": ["bot_token", "chat_id"],
            "properties": {
                "bot_token": { "type": "string", "title": "Bot token", "format": "password", "description": "From @BotFather." },
                "chat_id": { "type": "string", "title": "Chat id" },
                "silent": { "type": "boolean", "title": "Send without a notification sound", "default": false }
            }
        })
        .to_string()
    }

    fn send(event: Event, settings_json: String) -> Result<(), String> {
        let s: Settings = serde_json::from_str(&settings_json).map_err(|e| format!("bad settings: {e}"))?;
        if s.bot_token.trim().is_empty() || s.chat_id.trim().is_empty() {
            return Err("the bot token and chat id are needed".into());
        }
        let body = json!({ "chat_id": s.chat_id, "text": text(&event), "disable_notification": s.silent }).to_string();
        post(format!("https://api.telegram.org/bot{}/sendMessage", enc(s.bot_token.trim())), json_header(), body)
    }
}

export!(Plugin);
