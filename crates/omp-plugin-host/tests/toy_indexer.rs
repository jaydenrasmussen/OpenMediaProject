use std::{
    path::PathBuf,
    process::Command,
    sync::{Arc, OnceLock},
};

use anyhow::Result;
use omp_plugin_host::{Fetcher, Permissions, Plugin, PluginHost, SearchQuery};

struct FakeFetcher;

#[async_trait::async_trait]
impl Fetcher for FakeFetcher {
    async fn get(&self, _url: &str) -> Result<String> {
        Ok("hello".into())
    }
}

/// Builds the toy plugin once per test run and returns the .wasm path.
fn toy_wasm() -> &'static PathBuf {
    static WASM: OnceLock<PathBuf> = OnceLock::new();
    WASM.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/toy-indexer");
        // Put rustup's toolchain first so a system-installed rustc without the
        // wasm32-wasip2 target (e.g. Homebrew) cannot shadow it.
        let cargo = String::from_utf8(
            Command::new("rustup")
                .args(["which", "cargo"])
                .output()
                .expect("rustup is required to build plugins")
                .stdout,
        )
        .unwrap();
        let bin = PathBuf::from(cargo.trim()).parent().unwrap().to_path_buf();
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let status = Command::new(bin.join("cargo"))
            .args(["build", "--release", "--target", "wasm32-wasip2"])
            .current_dir(&dir)
            .env("PATH", path)
            .status()
            .unwrap();
        assert!(status.success(), "building toy-indexer failed");
        dir.join("target/wasm32-wasip2/release/toy_indexer.wasm")
    })
}

fn load(allowed_hosts: &[&str]) -> Plugin {
    PluginHost::new(Arc::new(FakeFetcher))
        .unwrap()
        .load(
            toy_wasm(),
            Permissions {
                allowed_hosts: allowed_hosts.iter().map(|s| s.to_string()).collect(),
            },
        )
        .unwrap()
}

fn q(term: &str) -> SearchQuery {
    SearchQuery {
        term: term.into(),
        season: None,
        episode: None,
    }
}

#[tokio::test]
async fn schema_and_search() {
    let plugin = load(&[]);
    let schema = plugin.settings_schema().await.unwrap();
    assert!(schema.contains("fetch_url"));

    let releases = plugin.search(&q("The Show"), "{}").await.unwrap();
    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0].title, "The Show 1080p WEB-DL");
}

#[tokio::test]
async fn http_allowed_host() {
    let plugin = load(&["indexer.test"]);
    let r = plugin
        .search(&q("x"), r#"{"fetch_url":"https://indexer.test/api"}"#)
        .await
        .unwrap();
    assert!(r[0].title.contains("[fetched 5 bytes]"), "{}", r[0].title);
}

#[tokio::test]
async fn http_denied_host() {
    let plugin = load(&["indexer.test"]);
    let r = plugin
        .search(&q("x"), r#"{"fetch_url":"https://evil.test/api"}"#)
        .await
        .unwrap();
    assert!(r[0].title.contains("not allowed"), "{}", r[0].title);
}

#[tokio::test]
async fn panic_is_contained_and_host_keeps_working() {
    let plugin = load(&[]);
    assert!(plugin.search(&q("panic"), "{}").await.is_err());
    // A fresh instance per call means the next call is unaffected.
    assert!(plugin.search(&q("ok"), "{}").await.is_ok());
}

#[tokio::test]
async fn runaway_cpu_is_stopped_by_fuel() {
    let plugin = load(&[]);
    let err = plugin.search(&q("spin"), "{}").await.unwrap_err();
    assert!(
        format!("{err:#}").to_lowercase().contains("fuel"),
        "{err:#}"
    );
}
