//! Reads a JSON array of movies from a URL.
//!
//! Each entry needs a TMDB id (`tmdbId`, `tmdb_id` or `id`); `title`, `year`
//! and `imdbId`/`imdb_id` are used when present. This is the shape Radarr's
//! "Custom List" and most list mirrors use.

wit_bindgen::generate!({
    path: "../../wit",
    world: "list-plugin",
});

use exports::omp::plugin::import_list::{Guest, Item};
use omp::plugin::host;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Settings {
    url: String,
}

struct CustomList;

fn number(v: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|k| match v.get(*k)? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    })
}

fn text(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| v.get(*k)?.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

impl Guest for CustomList {
    fn settings_schema() -> String {
        json!({
            "type": "object",
            "required": ["url"],
            "properties": {
                "url": { "type": "string", "title": "List URL", "description": "Must return a JSON array of movies." }
            }
        })
        .to_string()
    }

    fn fetch(settings_json: String) -> Result<Vec<Item>, String> {
        let settings: Settings =
            serde_json::from_str(&settings_json).map_err(|e| format!("bad settings: {e}"))?;
        if settings.url.trim().is_empty() {
            return Err("the URL is empty".into());
        }
        let body = host::http_get(settings.url.trim())?;
        let parsed: Value = serde_json::from_str(&body).map_err(|e| format!("the list isn't JSON: {e}"))?;
        let entries = parsed.as_array().ok_or("the list should be a JSON array")?;
        Ok(entries
            .iter()
            .filter_map(|e| {
                let tmdb_id = number(e, &["tmdbId", "tmdb_id", "id"])?;
                Some(Item {
                    tmdb_id,
                    title: text(e, &["title", "name"]).unwrap_or_else(|| format!("TMDB {tmdb_id}")),
                    year: number(e, &["year"]).and_then(|y| u32::try_from(y).ok()),
                    imdb_id: text(e, &["imdbId", "imdb_id"]),
                })
            })
            .collect())
    }
}

export!(CustomList);
