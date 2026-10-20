//! Posts every event as JSON to a configured URL.

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
    url: String,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
}

struct Webhook;

impl Guest for Webhook {
    fn settings_schema() -> String {
        json!({
            "type": "object",
            "required": ["url"],
            "properties": {
                "url": { "type": "string", "title": "URL", "format": "uri" },
                "method": { "type": "string", "title": "Method", "enum": ["POST", "PUT"], "default": "POST" },
                "username": { "type": "string", "title": "Username" },
                "password": { "type": "string", "title": "Password", "format": "password" }
            }
        })
        .to_string()
    }

    fn send(event: Event, settings_json: String) -> Result<(), String> {
        let s: Settings = serde_json::from_str(&settings_json).map_err(|e| format!("bad settings: {e}"))?;
        if s.url.trim().is_empty() {
            return Err("the URL is empty".into());
        }
        let body = json!({
            "event": event.kind,
            "title": event.title,
            "message": event.message,
            "movie": event.movie.map(|m| json!({
                "title": m.title,
                "year": m.year,
                "tmdbId": m.tmdb_id,
                "imdbId": m.imdb_id,
            })),
        })
        .to_string();

        let mut headers = vec![("content-type".to_owned(), "application/json".to_owned())];
        if let (Some(user), Some(pass)) = (s.username.as_deref(), s.password.as_deref())
            && !user.is_empty()
        {
            headers.push(("authorization".into(), format!("Basic {}", base64(&format!("{user}:{pass}")))));
        }
        let response = host::http(&HttpRequest {
            method: s.method.unwrap_or_else(|| "POST".into()),
            url: s.url,
            headers,
            body: Some(body),
        })?;
        if (200..300).contains(&response.status) {
            Ok(())
        } else {
            Err(format!("the server answered {}", response.status))
        }
    }
}

fn base64(input: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.as_bytes().chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

export!(Webhook);
