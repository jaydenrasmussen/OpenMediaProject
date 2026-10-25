//! Toy indexer plugin used to exercise the plugin host.
//!
//! `search` echoes the query back as one release. If the settings contain a
//! `fetch_url`, it also calls the host's `http-get` and reports the outcome in
//! the release title, so tests can see allow-list decisions.

wit_bindgen::generate!({
    path: "../../wit",
    world: "indexer-plugin",
});

use exports::omp::plugin::indexer::{Guest, Protocol, Release, SearchQuery};
use omp::plugin::host;

struct Toy;

fn release(title: String) -> Release {
    Release {
        guid: "toy-1".into(),
        title,
        size: 1_500_000_000,
        download_url: "https://example.invalid/toy-1.nzb".into(),
        publish_date: 1_700_000_000,
        protocol: Protocol::Usenet,
        seeders: None,
        leechers: None,
        info_hash: None,
        info_url: None,
        categories: vec![2040],
    }
}

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
            "memhog" => {
                // Far more than a plugin may hold.
                let hog = std::hint::black_box(vec![std::hint::black_box(1u8); 400 * 1024 * 1024]);
                let sum: u64 = hog.iter().step_by(4096).map(|b| u64::from(*b)).sum();
                return Ok(vec![release(format!("allocated {sum}"))]);
            }
            "requests" => {
                let mut refused = 0;
                for _ in 0..150 {
                    if let Err(e) = host::http_get("https://ok.example/") {
                        if e.contains("too many") {
                            refused += 1;
                        }
                    }
                }
                return Ok(vec![release(format!("refused {refused}"))]);
            }
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

        Ok(vec![release(title)])
    }

    fn recent(_settings_json: String) -> Result<Vec<Release>, String> {
        Ok(vec![release("Recent.Movie.2024.1080p.WEB-DL".into())])
    }

    fn test(settings_json: String) -> Result<(), String> {
        if settings_json.contains("\"broken\":true") {
            Err("toy plugin is broken".into())
        } else {
            Ok(())
        }
    }
}

export!(Toy);
