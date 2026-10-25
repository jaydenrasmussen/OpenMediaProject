//! The download-client plugins against the real programs, run in Docker.
//! Slow and needs Docker and the internet, so it only runs when asked:
//! `cargo test -p omp-plugin-host --test real_clients -- --ignored --nocapture --test-threads=1`

use std::{
    path::PathBuf,
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use omp_plugin_host::{
    ClientAddRequest, ClientStatus, DownloadClient, Fetcher, HttpRequest, HttpResponse, Manifest,
    Permissions, Plugin, PluginHost,
};
use serde_json::json;

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

struct Container(String);

impl Container {
    fn start(name: &str, image: &str, host_port: u16, inner_port: u16, env: &[&str]) -> Self {
        let name = format!("omp-real-{name}");
        sh(&["rm", "-f", &name]);
        let mut args = vec![
            "run".to_owned(),
            "-d".into(),
            "--name".into(),
            name.clone(),
            "--add-host".into(),
            "host.docker.internal:host-gateway".into(),
            "-p".into(),
            format!("{host_port}:{inner_port}"),
        ];
        for e in env {
            args.push("-e".into());
            args.push((*e).into());
        }
        args.push(image.into());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = sh(&refs);
        assert!(out.len() >= 12, "docker run failed: {out}");
        Self(name)
    }

    fn wait_http(&self, port: u16) {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(120) {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                std::thread::sleep(Duration::from_secs(4));
                return;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        panic!("{} never listened: {}", self.0, sh(&["logs", &self.0]));
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        sh(&["rm", "-f", &self.0]);
    }
}

fn load(name: &str) -> Plugin<DownloadClient> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins")
        .join(name);
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
    let wasm = dir
        .join("target/wasm32-wasip2/release")
        .join(&manifest.wasm);
    let client = omp_net::client_builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    PluginHost::new(Arc::new(Real(client)))
        .unwrap()
        .load_download_client(
            &wasm,
            Permissions {
                allowed_hosts: manifest.allowed_hosts,
            },
        )
        .unwrap()
}

fn request(protocol: &str, url: &str, hash: Option<&str>) -> ClientAddRequest {
    ClientAddRequest {
        title: "Real.Client.Test.2024.1080p".into(),
        url: url.into(),
        protocol: protocol.into(),
        category: Some("omp".into()),
        info_hash: hash.map(str::to_owned),
    }
}

/// The whole cycle against one running client.
async fn cycle(plugin: &Plugin<DownloadClient>, settings: &str, add: ClientAddRequest) {
    plugin.test(settings).await.expect("test connection");
    let id = plugin.add(&add, settings).await.expect("add");
    println!("added as {id}");

    let started = Instant::now();
    let item = loop {
        let items = plugin.items(settings).await.expect("items");
        if let Some(i) = items.into_iter().find(|i| i.id.eq_ignore_ascii_case(&id)) {
            break i;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "{id} never showed up"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    println!("seen: {} {:?}", item.title, item.status);
    assert!(!matches!(item.status, ClientStatus::Completed));

    plugin.pause(&id, settings).await.expect("pause");
    tokio::time::sleep(Duration::from_secs(2)).await;
    plugin.resume(&id, settings).await.expect("resume");
    plugin.remove(&id, true, settings).await.expect("remove");
    tokio::time::sleep(Duration::from_secs(2)).await;
    let left = plugin.items(settings).await.expect("items after remove");
    assert!(
        !left.iter().any(|i| i.id.eq_ignore_ascii_case(&id)),
        "{id} is still listed"
    );
}

const MAGNET: &str =
    "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Real.Client.Test";

/// Serves one tiny NZB so the usenet clients have something to fetch.
async fn serve_nzb() -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let nzb = r#"<?xml version="1.0" encoding="UTF-8"?><nzb xmlns="http://www.newzbin.com/DTD/2003/nzb"><file poster="a@b" date="1700000000" subject="Real.Client.Test.2024.1080p &quot;test.bin&quot; yEnc (1/1)"><groups><group>alt.binaries.test</group></groups><segments><segment bytes="100" number="1">abc123@example</segment></segments></file></nzb>"#;
    tokio::spawn(async move {
        loop {
            let (mut s, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/x-nzb\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    nzb.len()
                );
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(nzb.as_bytes()).await;
            });
        }
    });
    format!("http://host.docker.internal:{port}/Real.Client.Test.nzb")
}

#[tokio::test]
#[ignore = "needs Docker and the internet"]
async fn qbittorrent() {
    let c = Container::start(
        "qbt",
        "lscr.io/linuxserver/qbittorrent:latest",
        18401,
        // qBittorrent checks the Host header's port against its own.
        18401,
        &["WEBUI_PORT=18401"],
    );
    c.wait_http(18401);
    let logs = sh(&["logs", &c.0]);
    let password = logs
        .lines()
        .find_map(|l| l.split("session: ").nth(1))
        .map(|p| p.trim().to_owned())
        .unwrap_or_else(|| panic!("no temporary password in logs:\n{logs}"));
    let settings =
        json!({"server": "http://127.0.0.1:18401", "username": "admin", "password": password})
            .to_string();
    cycle(
        &load("qbittorrent"),
        &settings,
        request("torrent", MAGNET, None),
    )
    .await;
}

#[tokio::test]
#[ignore = "needs Docker and the internet"]
async fn transmission() {
    let c = Container::start(
        "tr",
        "lscr.io/linuxserver/transmission:latest",
        18402,
        9091,
        &[],
    );
    c.wait_http(18402);
    let settings = json!({"server": "http://127.0.0.1:18402"}).to_string();
    cycle(
        &load("transmission"),
        &settings,
        request("torrent", MAGNET, None),
    )
    .await;
}

#[tokio::test]
#[ignore = "needs Docker and the internet"]
async fn sabnzbd() {
    let c = Container::start(
        "sab",
        "lscr.io/linuxserver/sabnzbd:latest",
        18403,
        8080,
        &[],
    );
    c.wait_http(18403);
    let ini = sh(&["exec", &c.0, "cat", "/config/sabnzbd.ini"]);
    let key = ini
        .lines()
        .find_map(|l| l.strip_prefix("api_key = "))
        .map(str::trim)
        .unwrap_or_else(|| panic!("no api key:\n{ini}"))
        .to_owned();
    let settings = json!({"server": "http://localhost:18403", "api_key": key}).to_string();
    let nzb = serve_nzb().await;
    cycle(&load("sabnzbd"), &settings, request("usenet", &nzb, None)).await;
}

#[tokio::test]
#[ignore = "needs Docker and the internet"]
async fn nzbget() {
    let c = Container::start(
        "nzbget",
        "lscr.io/linuxserver/nzbget:latest",
        18404,
        6789,
        &[],
    );
    c.wait_http(18404);
    let settings =
        json!({"server": "http://127.0.0.1:18404", "username": "nzbget", "password": "tegbzn6789"})
            .to_string();
    let nzb = serve_nzb().await;
    cycle(&load("nzbget"), &settings, request("usenet", &nzb, None)).await;
}
