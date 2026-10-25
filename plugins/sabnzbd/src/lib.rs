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

fn api(settings: &Value, params: &[(&str, &str)]) -> Result<Value, String> {
    let mut pairs: Vec<(&str, &str)> = vec![("output", "json")];
    let key = setting(settings, "api_key");
    pairs.push(("apikey", &key));
    pairs.extend_from_slice(params);
    let response = request("GET", format!("{}/api?{}", server(settings)?, form(&pairs)), vec![], None)?;
    if response.status == 401 || response.status == 403 { return Err("SABnzbd refused the API key".into()); }
    if !(200..300).contains(&response.status) { return Err(format!("SABnzbd answered {}", response.status)); }
    let v: Value = serde_json::from_str(&response.body).map_err(|_| "that does not look like SABnzbd".to_owned())?;
    if let Some(e) = v.get("error").and_then(Value::as_str) { return Err(e.to_owned()); }
    Ok(v)
}

impl Guest for Plugin {
    fn settings_schema() -> String {
        json!({
            "type": "object",
            "required": ["server", "api_key"],
            "properties": {
                "server": { "type": "string", "title": "Server URL", "format": "uri", "default": "http://localhost:8080" },
                "api_key": { "type": "string", "title": "API key", "format": "password" }
            }
        }).to_string()
    }

    fn add(r: AddRequest, settings_json: String) -> Result<String, String> {
        let s = parse(&settings_json)?;
        let mut params = vec![("mode", "addurl"), ("name", r.url.as_str()), ("nzbname", r.title.as_str())];
        if let Some(c) = r.category.as_deref().filter(|c| !c.is_empty()) { params.push(("cat", c)); }
        let v = api(&s, &params)?;
        v.get("nzo_ids").and_then(|a| a.get(0)).and_then(Value::as_str).map(str::to_owned)
            .ok_or_else(|| "SABnzbd did not accept the NZB".to_owned())
    }

    fn items(settings_json: String) -> Result<Vec<Item>, String> {
        let s = parse(&settings_json)?;
        let mut out = Vec::new();
        let queue = api(&s, &[("mode", "queue")])?;
        let paused_all = queue["queue"]["paused"].as_bool().unwrap_or(false);
        for slot in queue["queue"]["slots"].as_array().cloned().unwrap_or_default() {
            let total = (slot["mb"].as_str().and_then(|m| m.parse::<f64>().ok()).unwrap_or(0.0) * 1048576.0) as u64;
            let left = (slot["mbleft"].as_str().and_then(|m| m.parse::<f64>().ok()).unwrap_or(0.0) * 1048576.0) as u64;
            let status = match text(&slot, "status").as_str() {
                "Paused" => Status::Paused,
                "Downloading" | "Fetching" | "Extracting" | "Verifying" | "Repairing" | "Moving" | "Running" => if paused_all { Status::Paused } else { Status::Downloading },
                _ => Status::Queued,
            };
            let eta = text(&slot, "timeleft").split(':').filter_map(|p| p.parse::<u64>().ok()).fold(0, |a, p| a * 60 + p);
            out.push(Item {
                id: text(&slot, "nzo_id"), title: text(&slot, "filename"),
                category: Some(text(&slot, "cat")).filter(|c| !c.is_empty() && c != "*"),
                total_size: total, remaining_size: left, status, output_path: None, seed_ratio: None,
                can_move_files: false, can_be_removed: false, message: None,
                eta_seconds: (eta > 0 && status == Status::Downloading).then_some(eta), download_speed: 0,
            });
        }
        let history = api(&s, &[("mode", "history"), ("limit", "100")])?;
        for slot in history["history"]["slots"].as_array().cloned().unwrap_or_default() {
            let failed = text(&slot, "status") == "Failed";
            let done = text(&slot, "status") == "Completed";
            if !failed && !done { continue; }
            let size = num(&slot, "bytes");
            out.push(Item {
                id: text(&slot, "nzo_id"), title: text(&slot, "name"),
                category: Some(text(&slot, "category")).filter(|c| !c.is_empty() && c != "*"),
                total_size: size, remaining_size: 0,
                status: if failed { Status::Failed } else { Status::Completed },
                output_path: Some(text(&slot, "storage")).filter(|p| !p.is_empty()), seed_ratio: None,
                can_move_files: done, can_be_removed: done,
                message: Some(text(&slot, "fail_message")).filter(|m| !m.is_empty()),
                eta_seconds: None, download_speed: 0,
            });
        }
        Ok(out)
    }

    fn remove(id: String, delete_data: bool, settings_json: String) -> Result<(), String> {
        let s = parse(&settings_json)?;
        let files = if delete_data { "1" } else { "0" };
        // It is in the queue or in the history; ask both.
        let q = api(&s, &[("mode", "queue"), ("name", "delete"), ("value", &id), ("del_files", files)]);
        let h = api(&s, &[("mode", "history"), ("name", "delete"), ("value", &id), ("del_files", files)]);
        q.and(h).map(|_| ()).or_else(|e| if e.contains("refused") { Err(e) } else { Ok(()) })
    }

    fn pause(id: String, settings_json: String) -> Result<(), String> {
        api(&parse(&settings_json)?, &[("mode", "queue"), ("name", "pause"), ("value", &id)]).map(|_| ())
    }

    fn resume(id: String, settings_json: String) -> Result<(), String> {
        api(&parse(&settings_json)?, &[("mode", "queue"), ("name", "resume"), ("value", &id)]).map(|_| ())
    }

    fn test(settings_json: String) -> Result<(), String> {
        let s = parse(&settings_json)?;
        // The version needs no key, the queue does: asking for both proves both.
        api(&s, &[("mode", "version")])?;
        api(&s, &[("mode", "queue"), ("limit", "1")]).map(|_| ())
    }
}

export!(Plugin);
