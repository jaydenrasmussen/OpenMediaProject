use std::{
    collections::HashMap,
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use omp_plugin_host::{
    Event, Fetcher, HttpRequest, HttpResponse, Indexer, MovieInfo, Notification, Permissions,
    Plugin, PluginHost, Protocol, SearchQuery, Storage,
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
        })
    }
}

/// An in-memory [`Storage`] for tests, scoped by `(plugin_id, key)` exactly
/// like the real SQLite-backed implementation.
#[derive(Default)]
struct FakeStorage {
    rows: Mutex<HashMap<(String, String), String>>,
}

#[async_trait::async_trait]
impl Storage for FakeStorage {
    async fn get(&self, plugin_id: &str, key: &str) -> Result<Option<String>> {
        Ok(self
            .rows
            .lock()
            .unwrap()
            .get(&(plugin_id.to_owned(), key.to_owned()))
            .cloned())
    }

    async fn set(&self, plugin_id: &str, key: &str, value: &str) -> Result<()> {
        self.rows
            .lock()
            .unwrap()
            .insert((plugin_id.to_owned(), key.to_owned()), value.to_owned());
        Ok(())
    }

    async fn delete(&self, plugin_id: &str, key: &str) -> Result<()> {
        self.rows
            .lock()
            .unwrap()
            .remove(&(plugin_id.to_owned(), key.to_owned()));
        Ok(())
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

fn ntfy_wasm() -> &'static PathBuf {
    static WASM: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    WASM.get_or_init(|| build_plugin("ntfy", "ntfy.wasm"))
}

fn gotify_wasm() -> &'static PathBuf {
    static WASM: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    WASM.get_or_init(|| build_plugin("gotify", "gotify.wasm"))
}

fn discord_wasm() -> &'static PathBuf {
    static WASM: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    WASM.get_or_init(|| build_plugin("discord", "discord.wasm"))
}

fn telegram_wasm() -> &'static PathBuf {
    static WASM: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    WASM.get_or_init(|| build_plugin("telegram", "telegram.wasm"))
}

fn mediaserver_scan_wasm() -> &'static PathBuf {
    static WASM: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    WASM.get_or_init(|| build_plugin("mediaserver-scan", "mediaserver_scan.wasm"))
}

fn permissions(hosts: &[&str]) -> Permissions {
    Permissions {
        allowed_hosts: hosts.iter().map(|s| s.to_string()).collect(),
    }
}

fn load(allowed_hosts: &[&str]) -> Plugin<Indexer> {
    PluginHost::new(Arc::new(FakeFetcher::default()))
        .unwrap()
        .load_indexer(toy_wasm(), "toy-indexer", permissions(allowed_hosts))
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
        .load_notification(webhook_wasm(), "webhook", permissions(hosts))
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

fn load_notification(
    wasm: &PathBuf,
    fetcher: Arc<FakeFetcher>,
    hosts: &[&str],
) -> Plugin<Notification> {
    load_notification_as(wasm, "notification-plugin", fetcher, hosts)
}

fn load_notification_as(
    wasm: &PathBuf,
    plugin_id: &str,
    fetcher: Arc<FakeFetcher>,
    hosts: &[&str],
) -> Plugin<Notification> {
    PluginHost::new(fetcher)
        .unwrap()
        .load_notification(wasm, plugin_id, permissions(hosts))
        .unwrap()
}

fn load_notification_with_storage(
    wasm: &PathBuf,
    plugin_id: &str,
    fetcher: Arc<FakeFetcher>,
    storage: Arc<FakeStorage>,
    hosts: &[&str],
) -> Plugin<Notification> {
    PluginHost::with_storage(fetcher, storage)
        .unwrap()
        .load_notification(wasm, plugin_id, permissions(hosts))
        .unwrap()
}

#[tokio::test]
async fn ntfy_posts_the_message_with_title_and_tags() {
    let fetcher = Arc::new(FakeFetcher::default());
    let plugin = load_notification(ntfy_wasm(), fetcher.clone(), &["*"]);
    assert!(
        plugin
            .settings_schema()
            .await
            .unwrap()
            .contains("\"topic\"")
    );

    plugin
        .send(
            &event(),
            r#"{"server":"https://ntfy.sh","topic":"omp","token":"tk_123","priority":"high"}"#,
        )
        .await
        .unwrap();
    let seen = fetcher.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let r = &seen[0];
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "https://ntfy.sh/omp")
    );
    assert_eq!(r.body.as_deref(), Some("The Matrix 1080p"));
    assert!(
        r.headers
            .iter()
            .any(|(k, v)| k == "title" && v == "Grabbed")
    );
    assert!(
        r.headers
            .iter()
            .any(|(k, v)| k == "tags" && v == "arrow_down")
    );
    assert!(
        r.headers
            .iter()
            .any(|(k, v)| k == "authorization" && v == "Bearer tk_123")
    );
}

#[tokio::test]
async fn ntfy_rejects_an_empty_topic() {
    let fetcher = Arc::new(FakeFetcher::default());
    let plugin = load_notification(ntfy_wasm(), fetcher.clone(), &["*"]);
    let err = plugin
        .send(&event(), r#"{"server":"https://ntfy.sh","topic":""}"#)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("topic"), "{err}");
    assert!(fetcher.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn gotify_posts_a_json_message_with_priority() {
    let fetcher = Arc::new(FakeFetcher::default());
    let plugin = load_notification(gotify_wasm(), fetcher.clone(), &["*"]);
    assert!(
        plugin
            .settings_schema()
            .await
            .unwrap()
            .contains("\"token\"")
    );

    plugin
        .send(
            &event(),
            r#"{"server":"https://gotify.example.com","token":"tok","priority":3}"#,
        )
        .await
        .unwrap();
    let seen = fetcher.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let r = &seen[0];
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "https://gotify.example.com/message?token=tok")
    );
    let body: serde_json::Value = serde_json::from_str(r.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["title"], "Grabbed");
    assert_eq!(body["message"], "The Matrix 1080p");
    assert_eq!(body["priority"], 3);
}

#[tokio::test]
async fn gotify_raises_priority_on_a_health_issue() {
    let fetcher = Arc::new(FakeFetcher::default());
    let plugin = load_notification(gotify_wasm(), fetcher.clone(), &["*"]);
    let mut e = event();
    e.kind = "health-issue".into();
    plugin
        .send(
            &e,
            r#"{"server":"https://gotify.example.com","token":"tok"}"#,
        )
        .await
        .unwrap();
    let seen = fetcher.seen.lock().unwrap();
    let body: serde_json::Value = serde_json::from_str(seen[0].body.as_deref().unwrap()).unwrap();
    assert_eq!(body["priority"], 8);
}

#[tokio::test]
async fn discord_posts_an_embed_with_movie_fields() {
    let fetcher = Arc::new(FakeFetcher::default());
    let plugin = load_notification(discord_wasm(), fetcher.clone(), &["discord.com"]);
    assert!(
        plugin
            .settings_schema()
            .await
            .unwrap()
            .contains("\"webhook_url\"")
    );

    plugin
        .send(
            &event(),
            r#"{"webhook_url":"https://discord.com/api/webhooks/1/abc"}"#,
        )
        .await
        .unwrap();
    let seen = fetcher.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let r = &seen[0];
    assert_eq!(r.method, "POST");
    let body: serde_json::Value = serde_json::from_str(r.body.as_deref().unwrap()).unwrap();
    let embed = &body["embeds"][0];
    assert_eq!(embed["title"], "Grabbed");
    assert_eq!(embed["description"], "The Matrix 1080p");
    let fields = embed["fields"].as_array().unwrap();
    assert!(
        fields
            .iter()
            .any(|f| f["name"] == "Title" && f["value"] == "The Matrix")
    );
}

#[tokio::test]
async fn discord_only_reaches_discord_hosts() {
    let fetcher = Arc::new(FakeFetcher::default());
    let restricted = load_notification(discord_wasm(), fetcher.clone(), &["discord.com"]);
    let err = restricted
        .send(&event(), r#"{"webhook_url":"https://evil.test/steal"}"#)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not allowed"), "{err}");
    assert!(fetcher.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn telegram_posts_markdown_v2_with_escaping() {
    let fetcher = Arc::new(FakeFetcher::default());
    let plugin = load_notification(telegram_wasm(), fetcher.clone(), &["api.telegram.org"]);
    assert!(
        plugin
            .settings_schema()
            .await
            .unwrap()
            .contains("\"bot_token\"")
    );

    plugin
        .send(&event(), r#"{"bot_token":"123:ABC","chat_id":"456"}"#)
        .await
        .unwrap();
    let seen = fetcher.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let r = &seen[0];
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "https://api.telegram.org/bot123:ABC/sendMessage")
    );
    let body: serde_json::Value = serde_json::from_str(r.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["chat_id"], "456");
    // "The Matrix 1080p" has a dot that MarkdownV2 requires escaped.
    assert!(
        body["text"].as_str().unwrap().contains("1080p"),
        "{}",
        body["text"]
    );
    assert_eq!(body["parse_mode"], "MarkdownV2");
}

#[tokio::test]
async fn telegram_rejects_an_empty_chat_id() {
    let fetcher = Arc::new(FakeFetcher::default());
    let plugin = load_notification(telegram_wasm(), fetcher.clone(), &["api.telegram.org"]);
    let err = plugin
        .send(&event(), r#"{"bot_token":"123:ABC","chat_id":""}"#)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("chat ID"), "{err}");
    assert!(fetcher.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn mediaserver_scan_refreshes_plex_by_token() {
    let fetcher = Arc::new(FakeFetcher::default());
    let plugin = load_notification(mediaserver_scan_wasm(), fetcher.clone(), &["*"]);
    let mut e = event();
    e.kind = "import".into();

    plugin
        .send(
            &e,
            r#"{"server_type":"plex","server":"http://nas:32400","plex_token":"ptk"}"#,
        )
        .await
        .unwrap();
    let seen = fetcher.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "GET");
    assert!(
        seen[0]
            .url
            .contains("http://nas:32400/library/sections/all/refresh")
    );
    assert!(seen[0].url.contains("X-Plex-Token=ptk"));
}

#[tokio::test]
async fn mediaserver_scan_refreshes_jellyfin_with_api_key_header() {
    let fetcher = Arc::new(FakeFetcher::default());
    let plugin = load_notification(mediaserver_scan_wasm(), fetcher.clone(), &["*"]);
    let mut e = event();
    e.kind = "upgrade".into();

    plugin
        .send(
            &e,
            r#"{"server_type":"jellyfin","server":"http://nas:8096","api_key":"jk"}"#,
        )
        .await
        .unwrap();
    let seen = fetcher.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let r = &seen[0];
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "http://nas:8096/Library/Refresh")
    );
    assert!(
        r.headers
            .iter()
            .any(|(k, v)| k == "x-emby-token" && v == "jk")
    );
}

#[tokio::test]
async fn mediaserver_scan_does_nothing_for_a_plain_grab_event() {
    let fetcher = Arc::new(FakeFetcher::default());
    let plugin = load_notification(mediaserver_scan_wasm(), fetcher.clone(), &["*"]);
    // `event()` defaults to kind "grab", which has not changed any file yet.
    plugin
        .send(
            &event(),
            r#"{"server_type":"plex","server":"http://nas:32400","plex_token":"ptk"}"#,
        )
        .await
        .unwrap();
    assert!(
        fetcher.seen.lock().unwrap().is_empty(),
        "a grab event should not trigger a library scan"
    );
}

#[tokio::test]
async fn mediaserver_scan_debounces_rapid_imports_using_storage() {
    let fetcher = Arc::new(FakeFetcher::default());
    let storage = Arc::new(FakeStorage::default());
    let plugin = load_notification_with_storage(
        mediaserver_scan_wasm(),
        "mediaserver-scan",
        fetcher.clone(),
        storage.clone(),
        &["*"],
    );
    let mut e = event();
    e.kind = "import".into();
    let settings = r#"{"server_type":"plex","server":"http://nas:32400","plex_token":"ptk","min_interval_seconds":3600}"#;

    // A season pack imports several episodes back to back: only the first
    // should actually ask Plex to rescan.
    plugin.send(&e, settings).await.unwrap();
    plugin.send(&e, settings).await.unwrap();
    plugin.send(&e, settings).await.unwrap();
    assert_eq!(
        fetcher.seen.lock().unwrap().len(),
        1,
        "later imports within the cooldown must not scan again"
    );
    assert!(
        storage.rows.lock().unwrap().contains_key(&(
            "mediaserver-scan".to_owned(),
            "last_scan:http://nas:32400".to_owned()
        )),
        "the last scan time was recorded under this server's key"
    );

    // The settings' Test button must always go through, even mid-cooldown.
    let mut test_event = event();
    test_event.kind = "test".into();
    plugin.send(&test_event, settings).await.unwrap();
    assert_eq!(fetcher.seen.lock().unwrap().len(), 2, "test always scans");

    // A different server is not affected by the first one's cooldown.
    let other = r#"{"server_type":"plex","server":"http://other:32400","plex_token":"ptk","min_interval_seconds":3600}"#;
    plugin.send(&e, other).await.unwrap();
    assert_eq!(
        fetcher.seen.lock().unwrap().len(),
        3,
        "a different server has its own cooldown"
    );

    // min_interval_seconds: 0 disables debouncing entirely.
    let no_debounce = r#"{"server_type":"plex","server":"http://nas:32400","plex_token":"ptk","min_interval_seconds":0}"#;
    plugin.send(&e, no_debounce).await.unwrap();
    assert_eq!(
        fetcher.seen.lock().unwrap().len(),
        4,
        "min_interval_seconds 0 always scans"
    );
}

#[tokio::test]
async fn mediaserver_scan_storage_is_private_to_this_plugin_id() {
    let fetcher = Arc::new(FakeFetcher::default());
    let storage = Arc::new(FakeStorage::default());
    // Pre-seed a recent scan time under a *different* plugin id, to show it
    // cannot leak into mediaserver-scan's own cooldown.
    storage
        .set(
            "some-other-plugin",
            "last_scan:http://nas:32400",
            "9999999999",
        )
        .await
        .unwrap();
    let plugin = load_notification_with_storage(
        mediaserver_scan_wasm(),
        "mediaserver-scan",
        fetcher.clone(),
        storage,
        &["*"],
    );
    let mut e = event();
    e.kind = "import".into();
    plugin
        .send(
            &e,
            r#"{"server_type":"plex","server":"http://nas:32400","plex_token":"ptk","min_interval_seconds":3600}"#,
        )
        .await
        .unwrap();
    assert_eq!(
        fetcher.seen.lock().unwrap().len(),
        1,
        "another plugin's stored value must not count as this plugin's cooldown"
    );
}
