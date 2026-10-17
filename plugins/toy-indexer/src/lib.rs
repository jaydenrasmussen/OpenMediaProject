//! Toy indexer plugin used to exercise the plugin host.
//!
//! `search` echoes the query back as one release. If the settings contain a
//! `fetch_url`, it also calls the host's `http-get` and reports the outcome in
//! the release title, so tests can see allow-list decisions.

wit_bindgen::generate!({
    path: "../../wit",
    world: "indexer-plugin",
});

use exports::omp::plugin::indexer::{Guest, Release, SearchQuery};
use omp::plugin::host;

struct Toy;

impl Guest for Toy {
    fn settings_schema() -> String {
        r#"{"type":"object","properties":{"fetch_url":{"type":"string"}}}"#.into()
    }

    fn search(query: SearchQuery, settings_json: String) -> Result<Vec<Release>, String> {
        host::log(host::LogLevel::Info, &format!("search {:?}", query.term));

        // Failure-mode triggers used by host tests.
        match query.term.as_str() {
            "panic" => panic!("toy plugin panic"),
            "spin" => loop {
                std::hint::black_box(());
            },
            _ => {}
        }

        let mut title = format!("{} 1080p WEB-DL", query.term);
        if let Some(url) = settings_json
            .split("\"fetch_url\":\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
        {
            match host::http_get(url) {
                Ok(body) => title = format!("{title} [fetched {} bytes]", body.len()),
                Err(e) => title = format!("{title} [fetch failed: {e}]"),
            }
        }

        Ok(vec![Release {
            guid: "toy-1".into(),
            title,
            size: 1_500_000_000,
            download_url: "https://example.invalid/toy-1.nzb".into(),
            publish_date: 1_700_000_000,
        }])
    }
}

export!(Toy);
