//! The Jellyfin library plugin against a real Jellyfin, run in Docker. Slow
//! and needs Docker and the internet, so it only runs when asked:
//! `cargo test -p omp-plugin-host --test real_media_servers -- --ignored --nocapture`
//!
//! A library of two titles named the way Jellyfin reads ids from folder names
//! (`[tmdbid-603]`) is made, scanned, and read back through the plugin. Plex
//! needs an account to claim a server, so it has no run like this one.

use std::{
    path::PathBuf,
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use omp_plugin_host::{
    Fetcher, HttpRequest, HttpResponse, Manifest, MediaItemKind, Permissions, PluginHost,
};
use serde_json::{Value, json};

struct Real(reqwest::Client);

#[async_trait::async_trait]
impl Fetcher for Real {
    async fn request(&self, r: HttpRequest) -> Result<HttpResponse> {
        let client = &self.0;
        let method = reqwest::Method::from_bytes(r.method.as_bytes())?;
        let mut req = client.request(method, &r.url);
        for (k, v) in &r.headers {
            req = req.header(k, v);
        }
        if let Some(b) = r.body {
            req = req.body(b);
        }
        let res = req.send().await?;
        let status = res.status().as_u16();
        let headers = res
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_owned(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                )
            })
            .collect();
        Ok(HttpResponse {
            status,
            body: res.text().await?,
            headers,
        })
    }
}

fn sh(args: &[&str]) -> String {
    let out = Command::new("docker").args(args).output().expect("docker");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

const AUTH: &str =
    r#"MediaBrowser Client="omp-test", Device="test", DeviceId="omp-test", Version="1""#;

async fn call(
    c: &reqwest::Client,
    method: reqwest::Method,
    url: String,
    token: Option<&str>,
    body: Option<Value>,
) -> (u16, String) {
    let auth = match token {
        Some(t) => format!(r#"{AUTH}, Token="{t}""#),
        None => AUTH.to_owned(),
    };
    let req = c.request(method, url).header("authorization", auth);
    let mut req = req;
    if let Some(b) = body {
        req = req
            .header("content-type", "application/json")
            .body(b.to_string());
    }
    let r = req.send().await.unwrap();
    (r.status().as_u16(), r.text().await.unwrap())
}

#[tokio::test]
#[ignore = "needs Docker and the internet; starts a real Jellyfin"]
async fn a_real_jellyfin_library_is_read_with_its_ids() {
    let name = "omp-real-jellyfin";
    let media = tempfile::tempdir().unwrap();
    let port = 18096;
    sh(&["rm", "-f", name]);
    let run = sh(&[
        "run",
        "-d",
        "--name",
        name,
        "-p",
        &format!("{port}:8096"),
        "-v",
        &format!("{}:/media", media.path().display()),
        "lscr.io/linuxserver/jellyfin",
    ]);
    assert!(run.len() >= 12, "docker run failed: {run}");
    struct Cleanup(&'static str);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            sh(&["rm", "-f", self.0]);
        }
    }
    let _cleanup = Cleanup("omp-real-jellyfin");

    let base = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();
    let started = Instant::now();
    loop {
        // The wizard's own endpoint answers only once start-up has finished.
        if let Ok(r) = http
            .get(format!("{base}/Startup/Configuration"))
            .header("authorization", AUTH)
            .send()
            .await
            && r.status().is_success()
        {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(180),
            "never came up: {}",
            sh(&["logs", name])
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    // Skip the first-run wizard.
    use reqwest::Method as M;
    call(&http, M::POST, format!("{base}/Startup/Configuration"), None, Some(json!({"UICulture": "en-US", "MetadataCountryCode": "US", "PreferredMetadataLanguage": "en"}))).await;
    call(&http, M::GET, format!("{base}/Startup/User"), None, None).await;
    call(
        &http,
        M::POST,
        format!("{base}/Startup/User"),
        None,
        Some(json!({"Name": "omp", "Password": "omp-test-pw"})),
    )
    .await;
    call(
        &http,
        M::POST,
        format!("{base}/Startup/RemoteAccess"),
        None,
        Some(json!({"EnableRemoteAccess": true, "EnableAutomaticPortMapping": false})),
    )
    .await;
    let (s, _) = call(
        &http,
        M::POST,
        format!("{base}/Startup/Complete"),
        None,
        None,
    )
    .await;
    assert!(s < 300, "wizard: {s}");
    let (s, auth) = call(
        &http,
        M::POST,
        format!("{base}/Users/AuthenticateByName"),
        None,
        Some(json!({"Username": "omp", "Pw": "omp-test-pw"})),
    )
    .await;
    assert_eq!(s, 200, "{auth}");
    let user_token = serde_json::from_str::<Value>(&auth).unwrap()["AccessToken"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        &http,
        M::POST,
        format!("{base}/Auth/Keys?app=omp"),
        Some(&user_token),
        None,
    )
    .await;
    let (_, keys) = call(
        &http,
        M::GET,
        format!("{base}/Auth/Keys"),
        Some(&user_token),
        None,
    )
    .await;
    let key = serde_json::from_str::<Value>(&keys).unwrap()["Items"][0]["AccessToken"]
        .as_str()
        .unwrap()
        .to_owned();

    // A movie and a show with two episodes. The files are tiny real videos.
    let ffmpeg = "/usr/lib/jellyfin-ffmpeg/ffmpeg";
    for (dir, file) in [
        (
            "movies/The Matrix (1999) [tmdbid-603]",
            "The Matrix (1999).mkv",
        ),
        (
            "tv/Breaking Bad (2008) [tmdbid-1396]/Season 01",
            "Breaking Bad S01E01.mkv",
        ),
        (
            "tv/Breaking Bad (2008) [tmdbid-1396]/Season 01",
            "Breaking Bad S01E02.mkv",
        ),
    ] {
        let path = format!("/media/{dir}/{file}");
        sh(&["exec", name, "mkdir", "-p", &format!("/media/{dir}")]);
        let out = sh(&[
            "exec",
            name,
            ffmpeg,
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=64x64:d=1",
            "-c:v",
            "mpeg4",
            &path,
        ]);
        assert!(out.contains("muxing overhead"), "ffmpeg: {out}");
    }
    for (name_, kind, path) in [
        ("Movies", "movies", "/media/movies"),
        ("Shows", "tvshows", "/media/tv"),
    ] {
        let (s, b) = call(
            &http,
            M::POST,
            format!("{base}/Library/VirtualFolders?name={name_}&collectionType={kind}&refreshLibrary=true"),
            Some(&key),
            Some(json!({"LibraryOptions": {"PathInfos": [{"Path": path}], "EnableInternetProviders": false}})),
        )
        .await;
        assert!(s < 300, "library {name_}: {s} {b}");
    }

    // Wait for the scan to see all four things.
    let started = Instant::now();
    loop {
        let (_, items) = call(
            &http,
            M::GET,
            format!("{base}/Items?Recursive=true&IncludeItemTypes=Movie,Series,Episode"),
            Some(&key),
            None,
        )
        .await;
        let v: Value = serde_json::from_str(&items).unwrap_or(Value::Null);
        let n = v["TotalRecordCount"].as_u64().unwrap_or(0);
        // An episode is listed before its numbers are read, so wait for those too.
        let numbered = v["Items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|i| {
                i["Type"] == "Episode" && i["IndexNumber"].is_u64() && i["SeriesId"].is_string()
            })
            .count();
        if n >= 4 && numbered == 2 {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(180),
            "the scan found {n} items: {}",
            sh(&["logs", "--tail", "40", name])
        );
        tokio::time::sleep(Duration::from_secs(3)).await;
    }

    // The plugin, as the app would run it.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/jellyfin-library");
    let cargo = String::from_utf8(
        Command::new("rustup")
            .args(["which", "cargo"])
            .output()
            .unwrap()
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
            .success()
    );
    let manifest = Manifest::parse(
        &std::fs::read_to_string(dir.join("plugin.toml")).unwrap(),
        &dir,
    )
    .unwrap();
    let plugin = PluginHost::new(Arc::new(Real(reqwest::Client::new())))
        .unwrap()
        .load_media_server(
            dir.join("target/wasm32-wasip2/release")
                .join(&manifest.wasm),
            Permissions {
                allowed_hosts: manifest.allowed_hosts.clone(),
            },
        )
        .unwrap();
    let settings = json!({ "url": base, "api_key": key }).to_string();

    let tested = plugin.test(&settings).await.unwrap();
    assert!(!tested.is_empty(), "the server names itself");
    let wrong = plugin
        .test(&json!({ "url": base, "api_key": "nope" }).to_string())
        .await
        .unwrap_err();
    assert!(wrong.to_string().contains("refused the API key"), "{wrong}");

    let mut items = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = plugin
            .library_page(cursor.as_deref(), &settings)
            .await
            .unwrap();
        items.extend(page.items);
        match page.next {
            Some(n) => cursor = Some(n),
            None => break,
        }
    }
    let movie = items
        .iter()
        .find(|i| i.kind == MediaItemKind::Movie)
        .expect("a movie");
    assert_eq!(
        (movie.title.as_str(), movie.year, movie.tmdb_id),
        ("The Matrix", Some(1999), Some(603))
    );
    let show = items
        .iter()
        .find(|i| i.kind == MediaItemKind::Series)
        .expect("a series");
    assert_eq!(
        (show.title.as_str(), show.tmdb_id),
        ("Breaking Bad", Some(1396))
    );
    let mut episodes: Vec<_> = items
        .iter()
        .filter(|i| i.kind == MediaItemKind::Episode)
        .map(|e| (e.series_key.clone(), e.season, e.episode))
        .collect();
    episodes.sort();
    assert_eq!(
        episodes,
        [
            (Some(show.key.clone()), Some(1), Some(1)),
            (Some(show.key.clone()), Some(1), Some(2)),
        ]
    );
}
