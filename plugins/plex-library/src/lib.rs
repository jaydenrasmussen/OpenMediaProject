//! Reads a Plex library, section by section. A movie section is read once;
//! a show section twice, first the shows and then their episodes.

wit_bindgen::generate!({
    path: "../../wit",
    world: "media-server-plugin",
});

use exports::omp::plugin::media_server::{Guest, ItemKind, LibraryItem, Page};
use omp::plugin::host::{self, HttpRequest};
use serde_json::{Value, json};

const PAGE: u32 = 100;

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().trim().to_owned()
}

fn parse(settings_json: &str) -> Result<(String, String), String> {
    let v: Value = serde_json::from_str(settings_json).map_err(|e| format!("bad settings: {e}"))?;
    let url = text(&v, "url").trim_end_matches('/').to_owned();
    let token = text(&v, "token");
    if url.is_empty() {
        return Err("the server address is empty".into());
    }
    if token.is_empty() {
        return Err("the Plex token is empty".into());
    }
    Ok((url, token))
}

fn get(url: String, token: &str, start: Option<u32>) -> Result<Value, String> {
    let mut headers = vec![("x-plex-token".into(), token.to_owned()), ("accept".into(), "application/json".into())];
    if let Some(start) = start {
        headers.push(("x-plex-container-start".into(), start.to_string()));
        headers.push(("x-plex-container-size".into(), PAGE.to_string()));
    }
    let r = host::http(&HttpRequest { method: "GET".into(), url, headers, body: None })?;
    match r.status {
        200..=299 => serde_json::from_str(&r.body).map_err(|e| format!("the server did not answer with JSON: {e}")),
        401 | 403 => Err("Plex refused the token".into()),
        s => Err(format!("Plex answered {s}")),
    }
}

/// What to read, in order: (section key, Plex type) where type is 1 movie, 2 show, 4 episode.
fn plan(sections: &Value) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    for d in sections["MediaContainer"]["Directory"].as_array().into_iter().flatten() {
        let key = text(d, "key");
        match text(d, "type").as_str() {
            "movie" => out.push((key, 1)),
            "show" => {
                out.push((key.clone(), 2));
                out.push((key, 4));
            }
            _ => {}
        }
    }
    out
}

fn guid(v: &Value, scheme: &str) -> Option<String> {
    let prefix = format!("{scheme}://");
    v.get("Guid")?
        .as_array()?
        .iter()
        .filter_map(|g| g.get("id").and_then(Value::as_str))
        .find_map(|id| id.strip_prefix(prefix.as_str()).map(str::to_owned))
}

fn item(v: &Value, plex_type: u32) -> LibraryItem {
    let n = |k: &str| v.get(k).and_then(Value::as_u64).map(|x| x as u32);
    let episode = plex_type == 4;
    LibraryItem {
        key: v.get("ratingKey").map(|k| k.as_str().map(str::to_owned).unwrap_or_else(|| k.to_string())).unwrap_or_default(),
        kind: match plex_type {
            1 => ItemKind::Movie,
            2 => ItemKind::Series,
            _ => ItemKind::Episode,
        },
        series_key: if episode { v.get("grandparentRatingKey").map(|k| k.as_str().map(str::to_owned).unwrap_or_else(|| k.to_string())) } else { None },
        title: if episode { text(v, "grandparentTitle") } else { text(v, "title") },
        year: n("year"),
        tmdb_id: guid(v, "tmdb").and_then(|s| s.parse().ok()),
        imdb_id: guid(v, "imdb"),
        tvdb_id: guid(v, "tvdb").and_then(|s| s.parse().ok()),
        season: if episode { n("parentIndex") } else { None },
        episode: if episode { n("index") } else { None },
    }
}

struct Plugin;

impl Guest for Plugin {
    fn settings_schema() -> String {
        json!({
            "type": "object",
            "required": ["url", "token"],
            "properties": {
                "url": {"type": "string", "title": "Server address", "description": "For example http://192.168.1.20:32400"},
                "token": {"type": "string", "title": "Plex token", "format": "password", "description": "Find it in Plex's help: Finding an authentication token (X-Plex-Token)."}
            }
        })
        .to_string()
    }

    /// The cursor is `section.type.offset`.
    fn library_page(cursor: Option<String>, settings_json: String) -> Result<Page, String> {
        let (url, token) = parse(&settings_json)?;
        let steps = plan(&get(format!("{url}/library/sections"), &token, None)?);
        let (mut at, offset) = match cursor.as_deref() {
            None => (0, 0),
            Some(c) => {
                let mut parts = c.split('.');
                let (section, kind, offset) = (parts.next(), parts.next().and_then(|k| k.parse::<u32>().ok()), parts.next().and_then(|o| o.parse::<u32>().ok()));
                let at = steps.iter().position(|(s, k)| Some(s.as_str()) == section && Some(*k) == kind);
                // A section that disappeared between pages ends the walk rather than restarting it.
                (at.ok_or("the library changed while it was being read; try again")?, offset.unwrap_or(0))
            }
        };
        let Some((section, kind)) = steps.get(at).cloned() else {
            return Ok(Page { items: Vec::new(), next: None });
        };
        let v = get(format!("{url}/library/sections/{section}/all?type={kind}&includeGuids=1"), &token, Some(offset))?;
        let container = &v["MediaContainer"];
        let raw = container["Metadata"].as_array().cloned().unwrap_or_default();
        let total = container["totalSize"].as_u64().unwrap_or(0) as u32;
        let read = offset + raw.len() as u32;
        let next = if !raw.is_empty() && read < total {
            Some(format!("{section}.{kind}.{read}"))
        } else {
            at += 1;
            steps.get(at).map(|(s, k)| format!("{s}.{k}.0"))
        };
        Ok(Page { items: raw.iter().map(|m| item(m, kind)).collect(), next })
    }

    fn test(settings_json: String) -> Result<String, String> {
        let (url, token) = parse(&settings_json)?;
        let v = get(format!("{url}/"), &token, None)?;
        let c = &v["MediaContainer"];
        Ok(format!("{} {}", text(c, "friendlyName"), text(c, "version")).trim().to_owned())
    }
}

export!(Plugin);
