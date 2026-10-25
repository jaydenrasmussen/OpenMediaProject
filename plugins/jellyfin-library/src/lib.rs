//! Reads a Jellyfin or Emby library. Both answer the same `/Items` query and
//! take the same API key.

wit_bindgen::generate!({
    path: "../../wit",
    world: "media-server-plugin",
});

use exports::omp::plugin::media_server::{Guest, ItemKind, LibraryItem, Page};
use omp::plugin::host::{self, HttpRequest};
use serde_json::{Value, json};

const PAGE: u32 = 200;

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().trim().to_owned()
}

fn parse(settings_json: &str) -> Result<(String, String), String> {
    let v: Value = serde_json::from_str(settings_json).map_err(|e| format!("bad settings: {e}"))?;
    let url = text(&v, "url").trim_end_matches('/').to_owned();
    let key = text(&v, "api_key");
    if url.is_empty() {
        return Err("the server address is empty".into());
    }
    if key.is_empty() {
        return Err("the API key is empty (create one under Dashboard, API Keys)".into());
    }
    Ok((url, key))
}

fn get(url: String, key: &str) -> Result<Value, String> {
    let r = host::http(&HttpRequest {
        method: "GET".into(),
        url,
        // Current Jellyfin wants the key in an Authorization header; Emby (and old Jellyfin) read X-Emby-Token.
        headers: vec![
            ("authorization".into(), format!(r#"MediaBrowser Client="OpenMediaProject", Device="server", DeviceId="openmediaproject", Version="1", Token="{key}""#)),
            ("x-emby-token".into(), key.to_owned()),
            ("accept".into(), "application/json".into()),
        ],
        body: None,
    })?;
    match r.status {
        200..=299 => serde_json::from_str(&r.body).map_err(|e| format!("the server did not answer with JSON: {e}")),
        401 | 403 => Err("the server refused the API key".into()),
        s => Err(format!("the server answered {s}")),
    }
}

fn id_of(provider_ids: &Value, name: &str) -> Option<String> {
    // Providers capitalise their keys differently ("Tmdb", "TMDB").
    provider_ids
        .as_object()?
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .and_then(|(_, v)| v.as_str())
        .map(str::to_owned)
        .filter(|s| !s.is_empty())
}

fn item(v: &Value) -> Option<LibraryItem> {
    let kind = match v.get("Type")?.as_str()? {
        "Movie" => ItemKind::Movie,
        "Series" => ItemKind::Series,
        "Episode" => ItemKind::Episode,
        _ => return None,
    };
    let ids = v.get("ProviderIds").cloned().unwrap_or(Value::Null);
    let n = |k: &str| v.get(k).and_then(Value::as_u64).map(|x| x as u32);
    Some(LibraryItem {
        key: text(v, "Id"),
        kind,
        series_key: Some(text(v, "SeriesId")).filter(|s| !s.is_empty() && kind == ItemKind::Episode),
        title: if kind == ItemKind::Episode { text(v, "SeriesName") } else { text(v, "Name") },
        year: n("ProductionYear"),
        tmdb_id: id_of(&ids, "Tmdb").and_then(|s| s.parse().ok()),
        imdb_id: id_of(&ids, "Imdb"),
        tvdb_id: id_of(&ids, "Tvdb").and_then(|s| s.parse().ok()),
        season: if kind == ItemKind::Episode { n("ParentIndexNumber") } else { None },
        episode: if kind == ItemKind::Episode { n("IndexNumber") } else { None },
    })
}

struct Plugin;

impl Guest for Plugin {
    fn settings_schema() -> String {
        json!({
            "type": "object",
            "required": ["url", "api_key"],
            "properties": {
                "url": {"type": "string", "title": "Server address", "description": "For example http://192.168.1.20:8096"},
                "api_key": {"type": "string", "title": "API key", "format": "password", "description": "Create one in the server's dashboard under API Keys."}
            }
        })
        .to_string()
    }

    fn library_page(cursor: Option<String>, settings_json: String) -> Result<Page, String> {
        let (url, key) = parse(&settings_json)?;
        let start: u32 = cursor.as_deref().map_or(Ok(0), str::parse).map_err(|_| "a bad page cursor".to_owned())?;
        let v = get(
            format!("{url}/Items?Recursive=true&IncludeItemTypes=Movie,Series,Episode&Fields=ProviderIds&EnableImages=false&EnableUserData=false&SortBy=SortName&StartIndex={start}&Limit={PAGE}"),
            &key,
        )?;
        let raw = v.get("Items").and_then(Value::as_array).ok_or("the server's answer has no Items")?;
        let total = v.get("TotalRecordCount").and_then(Value::as_u64).unwrap_or(0) as u32;
        let next = start + raw.len() as u32;
        Ok(Page {
            items: raw.iter().filter_map(item).collect(),
            next: (!raw.is_empty() && next < total).then(|| next.to_string()),
        })
    }

    fn test(settings_json: String) -> Result<String, String> {
        let (url, key) = parse(&settings_json)?;
        let v = get(format!("{url}/System/Info"), &key)?;
        let name = text(&v, "ServerName");
        let version = text(&v, "Version");
        Ok(format!("{name} {version}").trim().to_owned())
    }
}

export!(Plugin);
