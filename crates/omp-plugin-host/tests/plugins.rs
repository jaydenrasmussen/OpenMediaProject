use std::{
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use omp_plugin_host::{
    Event, Fetcher, HttpRequest, HttpResponse, Indexer, MovieInfo, Notification, Permissions,
    Plugin, PluginHost, Protocol, SearchQuery,
};

/// Answers every request with a canned response and remembers what was asked.
#[derive(Default)]
struct FakeFetcher {
    status: u16,
    seen: Mutex<Vec<HttpRequest>>,
}

#[async_trait::async_trait]
impl Fetcher for FakeFetcher {
    async fn request(&self, request: HttpRequest) -> Result<HttpResponse> {
        self.seen.lock().unwrap().push(request);
        Ok(HttpResponse {
            status: if self.status == 0 { 200 } else { self.status },
            body: "hello".into(),
            headers: Vec::new(),
        })
    }
}

/// Builds a plugin from `plugins/<name>` and returns its .wasm path.
fn build_plugin(name: &str, artifact: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins")
        .join(name);
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
    assert!(status.success(), "building {name} failed");
    dir.join("target/wasm32-wasip2/release").join(artifact)
}

fn toy_wasm() -> &'static PathBuf {
    static WASM: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    WASM.get_or_init(|| build_plugin("toy-indexer", "toy_indexer.wasm"))
}

fn webhook_wasm() -> &'static PathBuf {
    static WASM: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    WASM.get_or_init(|| build_plugin("webhook", "webhook.wasm"))
}

fn permissions(hosts: &[&str]) -> Permissions {
    Permissions {
        allowed_hosts: hosts.iter().map(|s| s.to_string()).collect(),
    }
}

fn load(allowed_hosts: &[&str]) -> Plugin<Indexer> {
    PluginHost::new(Arc::new(FakeFetcher::default()))
        .unwrap()
        .load_indexer(toy_wasm(), permissions(allowed_hosts))
        .unwrap()
}

fn q(term: &str) -> SearchQuery {
    SearchQuery {
        term: term.into(),
        imdb_id: None,
        tmdb_id: None,
        year: None,
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

#[tokio::test]
async fn memory_hogs_are_stopped() {
    let plugin = load(&[]);
    assert!(plugin.search(&q("memhog"), "{}").await.is_err());
    assert!(
        plugin.search(&q("ok"), "{}").await.is_ok(),
        "the host carries on"
    );
}

#[tokio::test]
async fn a_call_may_make_only_so_many_web_requests() {
    let plugin = load(&["ok.example"]);
    let found = plugin.search(&q("requests"), "{}").await.unwrap();
    assert!(found[0].title.contains("refused 50"), "{}", found[0].title);
}

#[tokio::test]
async fn indexer_recent_and_test_calls() {
    let plugin = load(&[]);
    let recent = plugin.recent("{}").await.unwrap();
    assert_eq!(recent[0].title, "Recent.Movie.2024.1080p.WEB-DL");
    assert_eq!(recent[0].protocol, Protocol::Usenet);
    assert!(plugin.test("{}").await.is_ok());
    let err = plugin.test(r#"{"broken":true}"#).await.unwrap_err();
    assert_eq!(err.to_string(), "toy plugin is broken");
}

fn webhook(fetcher: Arc<FakeFetcher>, hosts: &[&str]) -> Plugin<Notification> {
    PluginHost::new(fetcher)
        .unwrap()
        .load_notification(webhook_wasm(), permissions(hosts))
        .unwrap()
}

fn event() -> Event {
    Event {
        kind: "grab".into(),
        title: "Grabbed".into(),
        message: "The Matrix 1080p".into(),
        movie: Some(MovieInfo {
            title: "The Matrix".into(),
            year: 1999,
            tmdb_id: 603,
            imdb_id: Some("tt0133093".into()),
        }),
    }
}

#[tokio::test]
async fn webhook_posts_the_event_as_json() {
    let fetcher = Arc::new(FakeFetcher::default());
    let plugin = webhook(fetcher.clone(), &["*"]);
    assert!(plugin.settings_schema().await.unwrap().contains("\"url\""));

    plugin
        .send(
            &event(),
            r#"{"url":"https://hooks.test/in","username":"u","password":"p"}"#,
        )
        .await
        .unwrap();
    let seen = fetcher.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let r = &seen[0];
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "https://hooks.test/in")
    );
    let body: serde_json::Value = serde_json::from_str(r.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["event"], "grab");
    assert_eq!(body["movie"]["tmdbId"], 603);
    assert!(
        r.headers
            .iter()
            .any(|(k, v)| k == "authorization" && v == "Basic dTpw"),
        "{:?}",
        r.headers
    );
}

#[tokio::test]
async fn webhook_reports_failures() {
    let failing = Arc::new(FakeFetcher {
        status: 500,
        ..Default::default()
    });
    let err = webhook(failing, &["*"])
        .send(&event(), r#"{"url":"https://hooks.test/in"}"#)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("500"), "{err}");

    // Not on the allow-list, not a URL, bad settings.
    let fetcher = Arc::new(FakeFetcher::default());
    let restricted = webhook(fetcher.clone(), &["other.test"]);
    let err = restricted
        .send(&event(), r#"{"url":"https://hooks.test/in"}"#)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not allowed"), "{err}");
    assert!(
        restricted
            .send(&event(), r#"{"url":"file:///etc/passwd"}"#)
            .await
            .is_err()
    );
    assert!(restricted.send(&event(), "nonsense").await.is_err());
    assert!(
        fetcher.seen.lock().unwrap().is_empty(),
        "nothing reached the network"
    );
}
