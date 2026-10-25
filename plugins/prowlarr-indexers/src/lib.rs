//! Torrent sites from Prowlarr's indexer definitions, as a plugin. The same
//! engine the app has built in (`omp-cardigann`) runs here, in the sandbox,
//! over the host's HTTP.
//!
//! The host passes bodies as text, so a site that is not UTF-8 reads less
//! well, and a torrent file behind a sign-in cannot be fetched; sites that
//! give magnet links or direct downloads work fully.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use async_trait::async_trait;
use omp_cardigann::{Config, Http, HttpRequest, HttpResponse, Indexer, Library, Method, Query, QueryKind, Release};
use serde_json::{Value, json};

wit_bindgen::generate!({
    path: "../../wit",
    world: "indexer-plugin",
});

use exports::omp::plugin::indexer::{Guest, Protocol, Release as Found, SearchQuery};
use omp::plugin::host;

/// The host's HTTP, shaped for the engine.
struct HostHttp;

#[async_trait]
impl Http for HostHttp {
    async fn send(&self, r: HttpRequest) -> Result<HttpResponse, String> {
        let mut headers = r.headers.clone();
        let body = r.body.map(|(content_type, bytes)| {
            headers.push(("Content-Type".into(), content_type));
            String::from_utf8_lossy(&bytes).into_owned()
        });
        let response = host::http(&host::HttpRequest {
            method: match r.method {
                Method::Get => "GET",
                Method::Post => "POST",
            }
            .into(),
            url: r.url,
            headers,
            body,
        })?;
        Ok(HttpResponse {
            status: response.status,
            headers: response.headers,
            body: response.body.into_bytes(),
        })
    }
}

/// Host calls finish at once, so a future here is ready the first time it is asked.
fn block_on<F: Future>(f: F) -> Result<F::Output, String> {
    let mut f = pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => Ok(v),
        Poll::Pending => Err("the request did not finish".into()),
    }
}

fn indexer(settings_json: &str) -> Result<Indexer, String> {
    let s: Value = serde_json::from_str(settings_json).map_err(|e| format!("unreadable settings: {e}"))?;
    let key = s["definition"].as_str().filter(|d| !d.is_empty()).ok_or("choose a site")?;
    let def = Library::bundled_one(key).ok_or_else(|| format!("'{key}' is not one of the sites"))?;
    let mut config = Config::new();
    if let Some(base) = s["base_url"].as_str().filter(|b| !b.trim().is_empty()) {
        config.insert("base_url".into(), json!(base.trim()));
    }
    for name in ["username", "password", "cookie", "apikey", "passkey"] {
        if let Some(v) = s[name].as_str().filter(|v| !v.is_empty()) {
            config.insert(name.into(), json!(v));
        }
    }
    // Anything else the site asks for, one `name=value` to a line.
    for line in s["extra"].as_str().unwrap_or_default().lines() {
        if let Some((k, v)) = line.split_once('=') {
            config.insert(k.trim().to_owned(), json!(v.trim()));
        }
    }
    Indexer::new(def, config, Arc::new(HostHttp)).map_err(|e| e.to_string())
}

fn convert(r: Release) -> Option<Found> {
    let download_url = r.magnet.clone().or(r.download_url.clone()).or(r.details.clone())?;
    let seeders = r.seeders.map(|s| s.max(0) as u32);
    Some(Found {
        guid: r.guid,
        title: r.title,
        size: r.size,
        download_url,
        publish_date: r.publish_date.map_or(0, |d| d.timestamp()),
        protocol: Protocol::Torrent,
        seeders,
        leechers: r.peers.zip(seeders).map(|(p, s)| (p.max(0) as u32).saturating_sub(s)),
        info_hash: r.info_hash,
        info_url: r.details,
        categories: r.categories.into_iter().filter_map(|c| u32::try_from(c).ok()).collect(),
    })
}

fn run(settings_json: &str, q: Query) -> Result<Vec<Found>, String> {
    let idx = indexer(settings_json)?;
    let found = block_on(idx.search(&q))?.map_err(|e| e.to_string())?;
    Ok(found.into_iter().filter_map(convert).collect())
}

struct Plugin;

impl Guest for Plugin {
    fn settings_schema() -> String {
        json!({
            "type": "object",
            "required": ["definition"],
            "properties": {
                "definition": {"type": "string", "title": "Site", "description": "The site's name in Prowlarr's list", "enum": Library::bundled_names()},
                "base_url": {"type": "string", "title": "Address", "description": "Leave empty for the site's usual address"},
                "username": {"type": "string", "title": "User name"},
                "password": {"type": "string", "title": "Password", "format": "password"},
                "cookie": {"type": "string", "title": "Cookie", "description": "For sites that sign in with a browser cookie"},
                "apikey": {"type": "string", "title": "API key"},
                "passkey": {"type": "string", "title": "Passkey"},
                "extra": {"type": "string", "title": "Other settings", "description": "name=value, one to a line"}
            }
        })
        .to_string()
    }

    fn search(query: SearchQuery, settings_json: String) -> Result<Vec<Found>, String> {
        run(
            &settings_json,
            Query {
                kind: QueryKind::Movie,
                q: query.term,
                year: query.year.map(|y| y as i32),
                imdb_id: query.imdb_id,
                tmdb_id: query.tmdb_id,
                categories: vec![2000],
                ..Default::default()
            },
        )
    }

    fn recent(settings_json: String) -> Result<Vec<Found>, String> {
        run(&settings_json, Query { categories: vec![2000], ..Default::default() })
    }

    fn test(settings_json: String) -> Result<(), String> {
        let idx = indexer(&settings_json)?;
        block_on(idx.test())?.map_err(|e| e.to_string())
    }
}

export!(Plugin);
