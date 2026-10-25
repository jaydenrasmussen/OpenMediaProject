//! The torrent-site plugin (the Prowlarr definitions engine, built to WebAssembly)
//! run in the real host against a scripted site.

use std::{
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use omp_plugin_host::{
    Fetcher, HttpRequest, HttpResponse, Indexer, Manifest, Permissions, Plugin, PluginHost,
    SearchQuery,
};
use serde_json::{Value, json};

struct Site {
    seen: Mutex<Vec<HttpRequest>>,
}

#[async_trait::async_trait]
impl Fetcher for Site {
    async fn request(&self, request: HttpRequest) -> Result<HttpResponse> {
        self.seen.lock().unwrap().push(request.clone());
        // The Pirate Bay's API answers with a JSON list.
        Ok(HttpResponse {
            status: 200,
            body: json!([
                {"id": "1", "name": "Ubuntu 22.04 LTS", "info_hash": "2c6b6858d61da9543d4231a71db4b1c9264b0685", "leechers": "3", "seeders": "37", "num_files": "1", "size": "3654957056", "username": "x", "added": "1700000000", "status": "vip", "category": "207", "imdb": ""}
            ])
            .to_string(),
            headers: vec![("content-type".into(), "application/json".into())],
        })
    }
}

fn load() -> (Plugin<Indexer>, Arc<Site>) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/prowlarr-indexers");
    let cargo = String::from_utf8(
        Command::new("rustup")
            .args(["which", "cargo"])
            .output()
            .expect("rustup")
            .stdout,
    )
    .unwrap();
    let bin = PathBuf::from(cargo.trim()).parent().unwrap().to_path_buf();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    assert!(
        Command::new(bin.join("cargo"))
            .args(["build", "--release", "--target", "wasm32-wasip2"])
            .current_dir(&dir)
            .env("PATH", path)
            .status()
            .unwrap()
            .success(),
        "building the plugin"
    );
    let manifest = Manifest::parse(
        &std::fs::read_to_string(dir.join("plugin.toml")).unwrap(),
        &dir,
    )
    .unwrap();
    let site = Arc::new(Site {
        seen: Mutex::new(vec![]),
    });
    let plugin = PluginHost::new(site.clone())
        .unwrap()
        .load_indexer(
            dir.join("target/wasm32-wasip2/release")
                .join(&manifest.wasm),
            Permissions {
                allowed_hosts: manifest.allowed_hosts.clone(),
            },
        )
        .unwrap();
    (plugin, site)
}

#[tokio::test]
async fn a_public_site_is_searched_from_inside_the_sandbox() {
    let (plugin, site) = load();

    let schema: Value = serde_json::from_str(&plugin.settings_schema().await.unwrap()).unwrap();
    let sites = schema["properties"]["definition"]["enum"]
        .as_array()
        .unwrap();
    assert!(sites.len() > 500 && sites.iter().any(|s| s == "thepiratebay"));

    let settings = json!({"definition": "thepiratebay"}).to_string();
    let found = plugin
        .search(
            &SearchQuery {
                term: "ubuntu".into(),
                imdb_id: None,
                tmdb_id: None,
                year: None,
            },
            &settings,
        )
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    let r = &found[0];
    assert_eq!(r.title, "Ubuntu 22.04 LTS");
    assert_eq!(r.size, 3_654_957_056);
    assert_eq!((r.seeders, r.leechers), (Some(37), Some(3)));
    assert!(
        r.download_url
            .starts_with("magnet:?xt=urn:btih:2c6b6858d61da9543d4231a71db4b1c9264b0685")
    );
    assert_eq!(r.publish_date, 1_700_000_000);
    let first_url = site.seen.lock().unwrap()[0].url.clone();
    assert!(
        first_url.contains("apibay.org/q.php?q=ubuntu"),
        "{first_url}"
    );

    // Without a site chosen, or with one that is not in the list, it says so.
    let err = plugin
        .search(
            &SearchQuery {
                term: "x".into(),
                imdb_id: None,
                tmdb_id: None,
                year: None,
            },
            "{}",
        )
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("choose a site"), "{err:#}");
    let err = plugin
        .test(&json!({"definition": "nosuchsite"}).to_string())
        .await
        .unwrap_err();
    assert!(
        format!("{err:#}").contains("not one of the sites"),
        "{err:#}"
    );
}
