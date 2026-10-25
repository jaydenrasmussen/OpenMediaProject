//! The first-party notification plugins, built from source and run in the
//! real host against a fetcher that records what each one sends.

use std::{
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use omp_plugin_host::{
    Event, Fetcher, HttpRequest, HttpResponse, Manifest, Permissions, PluginHost,
};

#[derive(Default)]
struct Recorder {
    status: Mutex<u16>,
    seen: Mutex<Vec<HttpRequest>>,
}

#[async_trait::async_trait]
impl Fetcher for Recorder {
    async fn request(&self, request: HttpRequest) -> Result<HttpResponse> {
        self.seen.lock().unwrap().push(request);
        let status = *self.status.lock().unwrap();
        Ok(HttpResponse {
            status: if status == 0 { 200 } else { status },
            body: "{}".into(),
            headers: Vec::new(),
        })
    }
}

fn plugin_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins")
        .join(name)
}

fn build(name: &str) -> (Manifest, PathBuf) {
    let dir = plugin_dir(name);
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
        "building {name}"
    );
    let manifest = Manifest::parse(
        &std::fs::read_to_string(dir.join("plugin.toml")).unwrap(),
        &dir,
    )
    .unwrap();
    let wasm = dir
        .join("target/wasm32-wasip2/release")
        .join(&manifest.wasm);
    (manifest, wasm)
}

fn event(kind: &str) -> Event {
    Event {
        kind: kind.into(),
        title: "Release grabbed".into(),
        message: "Some Show S01E01 (WEBDL-1080p)".into(),
        movie: None,
    }
}

/// Runs one event through `name` and returns what it asked the web for.
async fn run(
    name: &str,
    settings: &str,
    kind: &str,
    status: u16,
) -> (Result<()>, Vec<HttpRequest>) {
    let (manifest, wasm) = build(name);
    let recorder = Arc::new(Recorder::default());
    *recorder.status.lock().unwrap() = status;
    let plugin = PluginHost::new(recorder.clone())
        .unwrap()
        .load_notification(
            &wasm,
            Permissions {
                allowed_hosts: manifest.allowed_hosts.clone(),
            },
        )
        .unwrap();
    let schema = plugin.settings_schema().await.unwrap();
    assert!(
        serde_json::from_str::<serde_json::Value>(&schema).is_ok(),
        "{name}: the schema is JSON"
    );
    let result = plugin.send(&event(kind), settings).await;
    let seen = recorder.seen.lock().unwrap().clone();
    (result, seen)
}

fn header<'a>(r: &'a HttpRequest, name: &str) -> Option<&'a str> {
    r.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

#[tokio::test]
async fn discord_and_slack_post_to_their_webhooks() {
    let (r, seen) = run(
        "discord",
        r#"{"webhook_url":"https://discord.com/api/webhooks/1/x","username":"Bot"}"#,
        "grab",
        204,
    )
    .await;
    r.unwrap();
    assert_eq!(seen[0].url, "https://discord.com/api/webhooks/1/x");
    let body: serde_json::Value = serde_json::from_str(seen[0].body.as_deref().unwrap()).unwrap();
    assert_eq!(body["username"], "Bot");
    assert_eq!(body["embeds"][0]["title"], "Release grabbed");
    assert!(
        run(
            "discord",
            r#"{"webhook_url":"http://discord.com/x"}"#,
            "grab",
            200
        )
        .await
        .0
        .is_err(),
        "https only"
    );

    let (r, seen) = run(
        "slack",
        r#"{"webhook_url":"https://hooks.slack.com/services/T/B/x"}"#,
        "grab",
        200,
    )
    .await;
    r.unwrap();
    assert!(
        seen[0]
            .body
            .as_deref()
            .unwrap()
            .contains("Some Show S01E01")
    );
}

#[tokio::test]
async fn telegram_ntfy_gotify_and_pushover_build_the_right_requests() {
    let (r, seen) = run(
        "telegram",
        r#"{"bot_token":"123:abc","chat_id":"42","silent":true}"#,
        "import",
        200,
    )
    .await;
    r.unwrap();
    assert_eq!(
        seen[0].url,
        "https://api.telegram.org/bot123%3Aabc/sendMessage"
    );
    let body: serde_json::Value = serde_json::from_str(seen[0].body.as_deref().unwrap()).unwrap();
    assert_eq!(
        (
            body["chat_id"].as_str(),
            body["disable_notification"].as_bool()
        ),
        (Some("42"), Some(true))
    );

    let (r, seen) = run(
        "ntfy",
        r#"{"server":"https://ntfy.example/","topic":"my topic","access_token":"tk","priority":4}"#,
        "grab",
        200,
    )
    .await;
    r.unwrap();
    assert_eq!(seen[0].url, "https://ntfy.example/my%20topic");
    assert_eq!(header(&seen[0], "authorization"), Some("Bearer tk"));
    assert_eq!(header(&seen[0], "priority"), Some("4"));
    assert_eq!(
        seen[0].body.as_deref(),
        Some("Some Show S01E01 (WEBDL-1080p)")
    );

    let (r, seen) = run(
        "gotify",
        r#"{"server":"https://gotify.example","app_token":"AppTok"}"#,
        "grab",
        200,
    )
    .await;
    r.unwrap();
    assert_eq!(seen[0].url, "https://gotify.example/message");
    assert_eq!(header(&seen[0], "x-gotify-key"), Some("AppTok"));

    let (r, seen) = run(
        "pushover",
        r#"{"user_key":"u","api_token":"t","priority":9}"#,
        "grab",
        200,
    )
    .await;
    r.unwrap();
    assert_eq!(
        header(&seen[0], "content-type"),
        Some("application/x-www-form-urlencoded")
    );
    let form = seen[0].body.as_deref().unwrap();
    assert!(
        form.contains("token=t&user=u&title=Release%20grabbed") && form.ends_with("priority=1"),
        "{form}"
    );
}

#[tokio::test]
async fn media_servers_are_asked_to_scan() {
    let (r, seen) = run(
        "plex",
        r#"{"server":"http://plex.lan:32400/","token":"PT"}"#,
        "import",
        200,
    )
    .await;
    r.unwrap();
    assert_eq!(
        seen[0].url,
        "http://plex.lan:32400/library/sections/all/refresh"
    );
    assert_eq!(header(&seen[0], "x-plex-token"), Some("PT"));
    let (_, test) = run(
        "plex",
        r#"{"server":"http://plex.lan:32400","token":"PT"}"#,
        "test",
        200,
    )
    .await;
    assert!(
        test[0].url.ends_with("/library/sections"),
        "a test only checks the connection"
    );

    let (r, seen) = run(
        "jellyfin-emby",
        r#"{"server":"http://jf.lan:8096","api_key":"K"}"#,
        "import",
        204,
    )
    .await;
    r.unwrap();
    assert_eq!(
        (seen[0].method.as_str(), seen[0].url.as_str()),
        ("POST", "http://jf.lan:8096/Library/Refresh")
    );
    assert_eq!(header(&seen[0], "x-emby-token"), Some("K"));
}

#[tokio::test]
async fn failures_and_missing_settings_are_reported() {
    let (r, _) = run("ntfy", r#"{"topic":"t"}"#, "grab", 500).await;
    assert!(format!("{:#}", r.unwrap_err()).contains("500"));
    for (name, settings) in [
        ("telegram", r#"{"bot_token":"","chat_id":""}"#),
        ("gotify", r#"{"server":"","app_token":""}"#),
        ("plex", r#"{"server":"","token":""}"#),
    ] {
        let (r, seen) = run(name, settings, "grab", 200).await;
        assert!(r.is_err() && seen.is_empty(), "{name}");
    }
}
