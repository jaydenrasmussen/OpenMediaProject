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

/// One RPC call. Transmission answers 409 with a session id the first time.
fn rpc(settings: &Value, method: &str, arguments: Value) -> Result<Value, String> {
    let base = server(settings)?;
    let url = if base.ends_with("/rpc") { base } else { format!("{base}/transmission/rpc") };
    let body = json!({ "method": method, "arguments": arguments }).to_string();
    let mut headers = vec![("content-type".to_owned(), "application/json".to_owned())];
    if let Some(a) = basic(&setting(settings, "username"), &setting(settings, "password")) { headers.push(a); }
    let mut response = request("POST", url.clone(), headers.clone(), Some(body.clone()))?;
    if response.status == 409 {
        let id = response.headers.iter().find(|(k, _)| k == "x-transmission-session-id").map(|(_, v)| v.clone()).ok_or("Transmission sent no session id")?;
        headers.push(("x-transmission-session-id".into(), id));
        response = request("POST", url, headers, Some(body))?;
    }
    if response.status == 401 { return Err("Transmission refused the username or password".into()); }
    if !(200..300).contains(&response.status) { return Err(format!("Transmission answered {}", response.status)); }
    let v: Value = serde_json::from_str(&response.body).map_err(|_| "that does not look like Transmission".to_owned())?;
    if v["result"].as_str() != Some("success") { return Err(v["result"].as_str().unwrap_or("Transmission refused the request").to_owned()); }
    Ok(v["arguments"].clone())
}

impl Guest for Plugin {
    fn settings_schema() -> String {
        json!({
            "type": "object",
            "required": ["server"],
            "properties": {
                "server": { "type": "string", "title": "Server URL", "format": "uri", "default": "http://localhost:9091" },
                "username": { "type": "string", "title": "Username" },
                "password": { "type": "string", "title": "Password", "format": "password" }
            }
        }).to_string()
    }

    fn add(r: AddRequest, settings_json: String) -> Result<String, String> {
        let s = parse(&settings_json)?;
        let mut args = json!({ "filename": r.url });
        if let Some(c) = r.category.as_deref().filter(|c| !c.is_empty()) { args["labels"] = json!([c]); }
        let out = rpc(&s, "torrent-add", args)?;
        let t = out.get("torrent-added").or_else(|| out.get("torrent-duplicate")).ok_or("Transmission did not accept the torrent")?;
        Ok(text(t, "hashString").to_uppercase())
    }

    fn items(settings_json: String) -> Result<Vec<Item>, String> {
        let s = parse(&settings_json)?;
        let out = rpc(&s, "torrent-get", json!({ "fields": ["hashString", "name", "totalSize", "leftUntilDone", "status", "downloadDir", "error", "errorString", "uploadRatio", "eta", "rateDownload", "labels", "metadataPercentComplete"] }))?;
        Ok(out["torrents"].as_array().cloned().unwrap_or_default().iter().map(|t| {
            let left = num(t, "leftUntilDone");
            let code = t["status"].as_i64().unwrap_or(0);
            let failed = t["error"].as_i64().unwrap_or(0) != 0;
            // A magnet link has no size until its metadata arrives; that is not "done".
            let have_metadata = t["metadataPercentComplete"].as_f64().unwrap_or(1.0) >= 1.0;
            let done = left == 0 && have_metadata;
            let status = if failed { Status::Failed } else if done { Status::Completed } else {
                match code { 0 => Status::Paused, 1 | 3 | 5 => Status::Queued, _ => Status::Downloading }
            };
            let eta = t["eta"].as_i64().filter(|e| *e > 0).map(|e| e as u64);
            let stopped_done = done && code == 0;
            Item {
                id: text(t, "hashString").to_uppercase(), title: text(t, "name"),
                category: t["labels"].as_array().and_then(|l| l.first()).and_then(Value::as_str).map(str::to_owned),
                total_size: num(t, "totalSize"), remaining_size: left, status,
                output_path: Some(format!("{}/{}", text(t, "downloadDir").trim_end_matches('/'), text(t, "name"))),
                seed_ratio: t.get("uploadRatio").and_then(Value::as_f64).filter(|r| *r >= 0.0),
                can_move_files: stopped_done, can_be_removed: stopped_done,
                message: Some(text(t, "errorString")).filter(|m| failed && !m.is_empty()),
                eta_seconds: eta.filter(|_| status == Status::Downloading), download_speed: num(t, "rateDownload"),
            }
        }).collect())
    }

    fn remove(id: String, delete_data: bool, settings_json: String) -> Result<(), String> {
        rpc(&parse(&settings_json)?, "torrent-remove", json!({ "ids": [id.to_lowercase()], "delete-local-data": delete_data })).map(|_| ())
    }

    fn pause(id: String, settings_json: String) -> Result<(), String> {
        rpc(&parse(&settings_json)?, "torrent-stop", json!({ "ids": [id.to_lowercase()] })).map(|_| ())
    }

    fn resume(id: String, settings_json: String) -> Result<(), String> {
        rpc(&parse(&settings_json)?, "torrent-start", json!({ "ids": [id.to_lowercase()] })).map(|_| ())
    }

    fn test(settings_json: String) -> Result<(), String> {
        rpc(&parse(&settings_json)?, "session-get", json!({})).map(|_| ())
    }
}

export!(Plugin);
