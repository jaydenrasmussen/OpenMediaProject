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

fn call(settings: &Value, method: &str, params: Value) -> Result<Value, String> {
    let mut headers = vec![("content-type".to_owned(), "application/json".to_owned())];
    if let Some(a) = basic(&setting(settings, "username"), &setting(settings, "password")) { headers.push(a); }
    let body = json!({ "method": method, "params": params, "id": 1 }).to_string();
    let r = request("POST", format!("{}/jsonrpc", server(settings)?), headers, Some(body))?;
    if r.status == 401 { return Err("NZBGet refused the username or password".into()); }
    if !(200..300).contains(&r.status) { return Err(format!("NZBGet answered {}", r.status)); }
    let v: Value = serde_json::from_str(&r.body).map_err(|_| "that does not look like NZBGet".to_owned())?;
    if let Some(e) = v.get("error").filter(|e| !e.is_null()) { return Err(e["message"].as_str().unwrap_or("NZBGet reported an error").to_owned()); }
    Ok(v["result"].clone())
}

impl Guest for Plugin {
    fn settings_schema() -> String {
        json!({
            "type": "object",
            "required": ["server"],
            "properties": {
                "server": { "type": "string", "title": "Server URL", "format": "uri", "default": "http://localhost:6789" },
                "username": { "type": "string", "title": "Username", "default": "nzbget" },
                "password": { "type": "string", "title": "Password", "format": "password" }
            }
        }).to_string()
    }

    fn add(r: AddRequest, settings_json: String) -> Result<String, String> {
        let s = parse(&settings_json)?;
        let name = if r.title.ends_with(".nzb") { r.title.clone() } else { format!("{}.nzb", r.title) };
        // NZBGet fetches the NZB itself when given its URL as the content.
        let id = call(&s, "append", json!([name, r.url, r.category.unwrap_or_default(), 0, false, false, "", 0, "SCORE", []]))?;
        match id.as_i64() {
            Some(n) if n > 0 => Ok(n.to_string()),
            _ => Err("NZBGet did not accept the NZB".into()),
        }
    }

    fn items(settings_json: String) -> Result<Vec<Item>, String> {
        let s = parse(&settings_json)?;
        let mut out = Vec::new();
        for g in call(&s, "listgroups", json!([0]))?.as_array().cloned().unwrap_or_default() {
            let total = num(&g, "FileSizeMB") * 1048576;
            let left = num(&g, "RemainingSizeMB") * 1048576;
            let status = match text(&g, "Status").as_str() {
                "PAUSED" => Status::Paused,
                "QUEUED" => Status::Queued,
                _ => Status::Downloading,
            };
            out.push(Item {
                id: g["NZBID"].as_i64().unwrap_or(0).to_string(), title: text(&g, "NZBName"),
                category: Some(text(&g, "Category")).filter(|c| !c.is_empty()),
                total_size: total, remaining_size: left, status, output_path: None, seed_ratio: None,
                can_move_files: false, can_be_removed: false, message: None, eta_seconds: None, download_speed: 0,
            });
        }
        for h in call(&s, "history", json!([false]))?.as_array().cloned().unwrap_or_default() {
            let st = text(&h, "Status");
            let ok = st.starts_with("SUCCESS");
            let bad = st.starts_with("FAILURE") || st.starts_with("DELETED");
            if !ok && !bad { continue; }
            out.push(Item {
                id: h["NZBID"].as_i64().unwrap_or(0).to_string(), title: text(&h, "Name"),
                category: Some(text(&h, "Category")).filter(|c| !c.is_empty()),
                total_size: num(&h, "FileSizeMB") * 1048576, remaining_size: 0,
                status: if ok { Status::Completed } else { Status::Failed },
                output_path: Some(text(&h, "FinalDir")).filter(|p| !p.is_empty()).or_else(|| Some(text(&h, "DestDir")).filter(|p| !p.is_empty())),
                seed_ratio: None, can_move_files: ok, can_be_removed: ok,
                message: bad.then(|| st.clone()), eta_seconds: None, download_speed: 0,
            });
        }
        Ok(out)
    }

    fn remove(id: String, delete_data: bool, settings_json: String) -> Result<(), String> {
        let s = parse(&settings_json)?;
        let n: i64 = id.parse().map_err(|_| "not an NZBGet id".to_owned())?;
        let in_queue = call(&s, "listgroups", json!([0]))?.as_array().is_some_and(|a| a.iter().any(|g| g["NZBID"].as_i64() == Some(n)));
        let command = if in_queue { "GroupFinalDelete" } else if delete_data { "HistoryFinalDelete" } else { "HistoryDelete" };
        call(&s, "editqueue", json!([command, "", [n]])).map(|_| ())
    }

    fn pause(id: String, settings_json: String) -> Result<(), String> {
        let n: i64 = id.parse().map_err(|_| "not an NZBGet id".to_owned())?;
        call(&parse(&settings_json)?, "editqueue", json!(["GroupPause", "", [n]])).map(|_| ())
    }

    fn resume(id: String, settings_json: String) -> Result<(), String> {
        let n: i64 = id.parse().map_err(|_| "not an NZBGet id".to_owned())?;
        call(&parse(&settings_json)?, "editqueue", json!(["GroupResume", "", [n]])).map(|_| ())
    }

    fn test(settings_json: String) -> Result<(), String> {
        call(&parse(&settings_json)?, "version", json!([])).map(|_| ())
    }
}

export!(Plugin);
