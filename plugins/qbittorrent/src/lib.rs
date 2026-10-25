#![allow(dead_code)]

wit_bindgen::generate!({
    path: "../../wit",
    world: "download-client-plugin",
});

use exports::omp::plugin::download_client::{AddRequest, Guest, Item, Status};
use omp::plugin::host::{self, HttpRequest};
use serde_json::{Value, json};

fn request(method: &str, url: String, headers: Vec<(String, String)>, body: Option<String>) -> Result<host::HttpResponse, String> {
    host::http(&HttpRequest { method: method.into(), url, headers, body })
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

fn form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&")
}

fn base64(input: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.as_bytes().chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() { out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char); } else { out.push('='); }
        }
    }
    out
}

fn basic(user: &str, pass: &str) -> Option<(String, String)> {
    (!user.is_empty()).then(|| ("authorization".to_owned(), format!("Basic {}", base64(&format!("{user}:{pass}")))))
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn num(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f.max(0.0) as u64))).unwrap_or(0)
}

fn setting(settings: &Value, key: &str) -> String {
    text(settings, key).trim().to_owned()
}

fn parse(settings_json: &str) -> Result<Value, String> {
    serde_json::from_str(settings_json).map_err(|e| format!("bad settings: {e}"))
}

fn server(settings: &Value) -> Result<String, String> {
    let s = setting(settings, "server");
    if s.is_empty() { return Err("the server address is empty".into()); }
    Ok(s.trim_end_matches('/').to_owned())
}

struct Plugin;

/// Signs in and returns the headers every later call needs.
fn login(settings: &Value) -> Result<(String, Vec<(String, String)>), String> {
    let base = server(settings)?;
    let body = form(&[("username", &setting(settings, "username")), ("password", &setting(settings, "password"))]);
    let response = request("POST", format!("{base}/api/v2/auth/login"), vec![("content-type".into(), "application/x-www-form-urlencoded".into()), ("referer".into(), base.clone())], Some(body))?;
    if response.status == 403 { return Err("qBittorrent has locked this address out after failed logins".into()); }
    // Newer versions answer 401 to a wrong login; older ones say "Fails." with 200.
    if response.status == 401 { return Err("qBittorrent refused the username or password (or the address does not match the Host it expects)".into()); }
    if !(200..300).contains(&response.status) { return Err(format!("qBittorrent answered {}", response.status)); }
    if response.body.trim().eq_ignore_ascii_case("fails.") { return Err("qBittorrent refused the username or password".into()); }
    let cookie = response.headers.iter().filter(|(k, _)| k == "set-cookie").filter_map(|(_, v)| v.split(';').next()).find(|c| c.starts_with("SID=") || c.starts_with("QBT_SID")).map(str::to_owned);
    let mut headers = vec![("referer".to_owned(), base.clone())];
    if let Some(c) = cookie { headers.push(("cookie".into(), c)); }
    Ok((base, headers))
}

fn post(base: &str, headers: &[(String, String)], path: &str, pairs: &[(&str, &str)]) -> Result<host::HttpResponse, String> {
    let mut h = headers.to_vec();
    h.push(("content-type".into(), "application/x-www-form-urlencoded".into()));
    let r = request("POST", format!("{base}{path}"), h, Some(form(pairs)))?;
    if !(200..300).contains(&r.status) { return Err(format!("qBittorrent answered {} for {path}", r.status)); }
    Ok(r)
}

fn magnet_hash(url: &str) -> Option<String> {
    let rest = url.strip_prefix("magnet:?")?;
    let btih = rest.split('&').find_map(|p| p.strip_prefix("xt=urn:btih:"))?;
    if btih.len() == 40 && btih.chars().all(|c| c.is_ascii_hexdigit()) { Some(btih.to_uppercase()) } else { None }
}

impl Guest for Plugin {
    fn settings_schema() -> String {
        json!({
            "type": "object",
            "required": ["server"],
            "properties": {
                "server": { "type": "string", "title": "Server URL", "format": "uri", "default": "http://localhost:8080" },
                "username": { "type": "string", "title": "Username" },
                "password": { "type": "string", "title": "Password", "format": "password" }
            }
        }).to_string()
    }

    fn add(r: AddRequest, settings_json: String) -> Result<String, String> {
        let s = parse(&settings_json)?;
        let (base, headers) = login(&s)?;
        let known = r.info_hash.clone().or_else(|| magnet_hash(&r.url));
        // Without a hash, a tag unique to this add finds the torrent afterwards.
        let tag = format!("omp-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        let mut pairs = vec![("urls", r.url.as_str()), ("tags", tag.as_str())];
        if let Some(c) = r.category.as_deref().filter(|c| !c.is_empty()) { pairs.push(("category", c)); }
        let response = post(&base, &headers, "/api/v2/torrents/add", &pairs)?;
        if response.body.trim().eq_ignore_ascii_case("fails.") { return Err("qBittorrent did not accept the torrent".into()); }
        if let Some(hash) = known { return Ok(hash); }
        for _ in 0..10 {
            let list = request("GET", format!("{base}/api/v2/torrents/info?tag={}", enc(&tag)), headers.clone(), None)?;
            if let Ok(Value::Array(a)) = serde_json::from_str::<Value>(&list.body)
                && let Some(h) = a.first().and_then(|t| t.get("hash")).and_then(Value::as_str) {
                return Ok(h.to_uppercase());
            }
            // qBittorrent needs a moment to fetch the file.
            let _ = request("GET", format!("{base}/api/v2/app/version"), headers.clone(), None);
        }
        Err("qBittorrent took the torrent but did not report it".into())
    }

    fn items(settings_json: String) -> Result<Vec<Item>, String> {
        let s = parse(&settings_json)?;
        let (base, headers) = login(&s)?;
        let r = request("GET", format!("{base}/api/v2/torrents/info"), headers, None)?;
        if !(200..300).contains(&r.status) { return Err(format!("qBittorrent answered {}", r.status)); }
        let list: Vec<Value> = serde_json::from_str(&r.body).map_err(|e| format!("unreadable answer: {e}"))?;
        Ok(list.iter().map(|t| {
            let state = text(t, "state");
            let status = match state.as_str() {
                "error" | "missingFiles" => Status::Failed,
                "pausedDL" | "stoppedDL" => Status::Paused,
                "queuedDL" | "checkingDL" | "checkingResumeData" | "allocating" => Status::Queued,
                "downloading" | "metaDL" | "forcedMetaDL" | "stalledDL" | "forcedDL" => Status::Downloading,
                _ => Status::Completed,
            };
            let finished_stopped = matches!(state.as_str(), "pausedUP" | "stoppedUP");
            let eta = num(t, "eta");
            Item {
                id: text(t, "hash").to_uppercase(), title: text(t, "name"),
                category: Some(text(t, "category")).filter(|c| !c.is_empty()),
                total_size: num(t, "total_size").max(num(t, "size")), remaining_size: num(t, "amount_left"), status,
                output_path: Some(text(t, "content_path")).filter(|p| !p.is_empty()),
                seed_ratio: t.get("ratio").and_then(Value::as_f64),
                can_move_files: finished_stopped, can_be_removed: finished_stopped,
                message: (status == Status::Failed).then(|| state.clone()),
                eta_seconds: (eta > 0 && eta < 8640000 && status == Status::Downloading).then_some(eta), download_speed: num(t, "dlspeed"),
            }
        }).collect())
    }

    fn remove(id: String, delete_data: bool, settings_json: String) -> Result<(), String> {
        let (base, headers) = login(&parse(&settings_json)?)?;
        post(&base, &headers, "/api/v2/torrents/delete", &[("hashes", &id.to_lowercase()), ("deleteFiles", if delete_data { "true" } else { "false" })]).map(|_| ())
    }

    fn pause(id: String, settings_json: String) -> Result<(), String> {
        let (base, headers) = login(&parse(&settings_json)?)?;
        let hash = id.to_lowercase();
        // Version 5 renamed pause to stop.
        post(&base, &headers, "/api/v2/torrents/pause", &[("hashes", &hash)]).map(|_| ()).or_else(|_| post(&base, &headers, "/api/v2/torrents/stop", &[("hashes", &hash)]).map(|_| ()))
    }

    fn resume(id: String, settings_json: String) -> Result<(), String> {
        let (base, headers) = login(&parse(&settings_json)?)?;
        let hash = id.to_lowercase();
        post(&base, &headers, "/api/v2/torrents/resume", &[("hashes", &hash)]).map(|_| ()).or_else(|_| post(&base, &headers, "/api/v2/torrents/start", &[("hashes", &hash)]).map(|_| ()))
    }

    fn test(settings_json: String) -> Result<(), String> {
        let (base, headers) = login(&parse(&settings_json)?)?;
        let r = request("GET", format!("{base}/api/v2/app/version"), headers, None)?;
        if r.status == 403 { return Err("qBittorrent wants a login: check the username and password".into()); }
        if !(200..300).contains(&r.status) { return Err(format!("qBittorrent answered {}", r.status)); }
        Ok(())
    }
}

export!(Plugin);
