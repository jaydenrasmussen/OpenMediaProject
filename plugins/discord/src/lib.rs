//! Sends every event as an embed to a Discord channel webhook.
//! API: <https://discord.com/developers/docs/resources/webhook#execute-webhook>

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
    webhook_url: String,
}

struct Discord;

impl Guest for Discord {
    fn settings_schema() -> String {
        r#"{
            "type": "object",
            "required": ["webhook_url"],
            "properties": {
                "webhook_url": {
                    "type": "string",
                    "title": "Webhook URL",
                    "format": "uri",
                    "description": "Channel Settings → Integrations → Webhooks in Discord."
                }
            }
        }"#
        .to_owned()
    }

    fn send(event: Event, settings_json: String) -> Result<(), String> {
        let s: Settings =
            serde_json::from_str(&settings_json).map_err(|e| format!("bad settings: {e}"))?;
        let url = s.webhook_url.trim();
        if url.is_empty() {
            return Err("the webhook URL is empty".into());
        }

        let mut fields = Vec::new();
        if let Some(movie) = &event.movie {
            fields.push(json!({ "name": "Title", "value": movie.title, "inline": true }));
            fields.push(json!({ "name": "Year", "value": movie.year.to_string(), "inline": true }));
            if let Some(imdb) = &movie.imdb_id {
                fields.push(json!({
                    "name": "IMDb",
                    "value": format!("[{imdb}](https://www.imdb.com/title/{imdb}/)"),
                    "inline": true
                }));
            }
        }

        let body = json!({
            "embeds": [{
                "title": event.title,
                "description": event.message,
                "color": color_for(&event.kind),
                "fields": fields,
            }]
        })
        .to_string();

        let response = host::http(&HttpRequest {
            method: "POST".to_owned(),
            url: url.to_owned(),
            headers: vec![("content-type".to_owned(), "application/json".to_owned())],
            body: Some(body),
        })?;
        // Discord answers a plain webhook post with 204 No Content.
        if (200..300).contains(&response.status) {
            Ok(())
        } else {
            Err(format!(
                "Discord answered {}: {}",
                response.status, response.body
            ))
        }
    }
}

/// Discord embed colors are a packed 24-bit RGB integer.
fn color_for(kind: &str) -> u32 {
    match kind {
        "grab" => 0x3498DB,                             // blue
        "import" | "upgrade" => 0x2ECC71,               // green
        "download-failed" | "health-issue" => 0xE74C3C, // red
        "health-restored" => 0x2ECC71,                  // green
        _ => 0x95A5A6,                                  // grey
    }
}

export!(Discord);
