//! The download client plugins, built from source and run in the real host
//! against scripted stand-ins for each client's API.

use std::{
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use omp_plugin_host::{
    ClientAddRequest, ClientStatus, DownloadClient, Fetcher, HttpRequest, HttpResponse, Manifest,
    Permissions, Plugin, PluginHost,
};
use serde_json::{Value, json};

type Script = Box<dyn Fn(&HttpRequest) -> HttpResponse + Send + Sync>;

struct Scripted {
    script: Script,
    seen: Mutex<Vec<HttpRequest>>,
}

#[async_trait::async_trait]
impl Fetcher for Scripted {
    async fn request(&self, request: HttpRequest) -> Result<HttpResponse> {
        let response = (self.script)(&request);
        self.seen.lock().unwrap().push(request);
        Ok(response)
    }
}

fn reply(status: u16, body: impl Into<String>) -> HttpResponse {
    HttpResponse {
        status,
        body: body.into(),
        headers: Vec::new(),
    }
}

fn load(name: &str, script: Script) -> (Plugin<DownloadClient>, Arc<Scripted>, Manifest) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins")
        .join(name);
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
    let fetcher = Arc::new(Scripted {
        script,
        seen: Mutex::new(Vec::new()),
    });
    let plugin = PluginHost::new(fetcher.clone())
        .unwrap()
        .load_download_client(
            &wasm,
            Permissions {
                allowed_hosts: manifest.allowed_hosts.clone(),
            },
        )
        .unwrap();
    (plugin, fetcher, manifest)
}

fn add(url: &str, category: Option<&str>, hash: Option<&str>) -> ClientAddRequest {
    ClientAddRequest {
        title: "Some.Show.S01E01.1080p".into(),
        url: url.into(),
        protocol: "usenet".into(),
        category: category.map(str::to_owned),
        info_hash: hash.map(str::to_owned),
    }
}

/// A value from the URL's query or from a form body.
fn query(r: &HttpRequest, key: &str) -> Option<String> {
    let from_url = url::Url::parse(&r.url)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned());
    from_url.or_else(|| {
        r.body.as_deref().and_then(|b| {
            url::form_urlencoded::parse(b.as_bytes())
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.into_owned())
        })
    })
}

#[tokio::test]
async fn sabnzbd_adds_lists_and_controls() {
    let (plugin, seen, manifest) = load(
        "sabnzbd",
        Box::new(|r| {
            let mode = query(r, "mode").unwrap_or_default();
            if query(r, "apikey").as_deref() != Some("KEY") {
                return reply(200, r#"{"status":false,"error":"API Key Incorrect"}"#);
            }
            match mode.as_str() {
                "version" => reply(200, r#"{"version":"4.3"}"#),
                "addurl" => reply(200, r#"{"status":true,"nzo_ids":["SABnzbd_nzo_abc"]}"#),
                "queue" if query(r, "name").is_some() => reply(200, r#"{"status":true}"#),
                "queue" => reply(
                    200,
                    r#"{"queue":{"paused":false,"slots":[{"nzo_id":"SABnzbd_nzo_abc","filename":"Some.Show","cat":"tv","mb":"100.0","mbleft":"25.0","status":"Downloading","timeleft":"0:01:30"}]}}"#,
                ),
                "history" if query(r, "name").is_some() => reply(200, r#"{"status":true}"#),
                "history" => reply(
                    200,
                    r#"{"history":{"slots":[{"nzo_id":"nzo_done","name":"Done.One","category":"tv","bytes":2048,"status":"Completed","storage":"/dl/Done.One"},{"nzo_id":"nzo_bad","name":"Bad.One","category":"tv","bytes":10,"status":"Failed","fail_message":"par2 failed"},{"nzo_id":"nzo_x","name":"Extracting","category":"tv","bytes":10,"status":"Extracting"}]}}"#,
                ),
                _ => reply(404, ""),
            }
        }),
    );
    assert_eq!(manifest.protocol.as_deref(), Some("usenet"));
    let settings = json!({"server": "http://sab.lan:8080/", "api_key": "KEY"}).to_string();
    plugin.test(&settings).await.unwrap();
    assert!(
        plugin
            .test(&json!({"server": "http://sab.lan:8080", "api_key": "BAD"}).to_string())
            .await
            .unwrap_err()
            .to_string()
            .contains("API Key")
    );

    assert_eq!(
        plugin
            .add(
                &add("https://idx/get.nzb?id=1", Some("tv"), None),
                &settings
            )
            .await
            .unwrap(),
        "SABnzbd_nzo_abc"
    );
    let sent = seen
        .seen
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|r| query(r, "mode").as_deref() == Some("addurl"))
        .cloned()
        .unwrap();
    assert_eq!(
        (
            query(&sent, "name").unwrap(),
            query(&sent, "cat").unwrap(),
            query(&sent, "nzbname").unwrap()
        ),
        (
            "https://idx/get.nzb?id=1".into(),
            "tv".into(),
            "Some.Show.S01E01.1080p".into()
        )
    );

    let items = plugin.items(&settings).await.unwrap();
    assert_eq!(
        items.len(),
        3,
        "the unfinished history entry is not an item of its own"
    );
    let q = items.iter().find(|i| i.id == "SABnzbd_nzo_abc").unwrap();
    assert_eq!(
        (q.status, q.total_size, q.remaining_size, q.eta_seconds),
        (ClientStatus::Downloading, 104_857_600, 26_214_400, Some(90))
    );
    let done = items.iter().find(|i| i.id == "nzo_done").unwrap();
    assert_eq!(
        (
            done.status,
            done.output_path.as_deref(),
            done.can_be_removed
        ),
        (ClientStatus::Completed, Some("/dl/Done.One"), true)
    );
    let bad = items.iter().find(|i| i.id == "nzo_bad").unwrap();
    assert_eq!(
        (bad.status, bad.message.as_deref()),
        (ClientStatus::Failed, Some("par2 failed"))
    );

    plugin.pause("SABnzbd_nzo_abc", &settings).await.unwrap();
    plugin.resume("SABnzbd_nzo_abc", &settings).await.unwrap();
    plugin
        .remove("SABnzbd_nzo_abc", true, &settings)
        .await
        .unwrap();
    let names: Vec<String> = seen
        .seen
        .lock()
        .unwrap()
        .iter()
        .filter_map(|r| query(r, "name"))
        .collect();
    assert!(
        names.iter().any(|n| n == "pause")
            && names.iter().any(|n| n == "resume")
            && names.iter().any(|n| n == "delete")
    );
}

#[tokio::test]
async fn qbittorrent_signs_in_and_follows_a_torrent() {
    let (plugin, seen, _) = load(
        "qbittorrent",
        Box::new(|r| {
            let path = url::Url::parse(&r.url).unwrap().path().to_owned();
            if path == "/api/v2/auth/login" {
                let ok = r
                    .body
                    .as_deref()
                    .is_some_and(|b| b.contains("password=secret"));
                return HttpResponse {
                    // As current qBittorrent answers: 204 for a good login, 401 for a bad one.
                    status: if ok { 204 } else { 401 },
                    body: String::new(),
                    headers: if ok {
                        vec![(
                            "set-cookie".into(),
                            "QBT_SID_8080=abc123; HttpOnly; path=/".into(),
                        )]
                    } else {
                        vec![]
                    },
                };
            }
            // Everything else needs the cookie.
            if !r
                .headers
                .iter()
                .any(|(k, v)| k == "cookie" && v == "QBT_SID_8080=abc123")
            {
                return reply(403, "Forbidden");
            }
            match path.as_str() {
                "/api/v2/app/version" => reply(200, "v4.6.0"),
                "/api/v2/torrents/add" => reply(200, "Ok."),
                "/api/v2/torrents/info" if r.url.contains("tag=") => reply(
                    200,
                    r#"[{"hash":"aabbccddeeff00112233445566778899aabbccdd"}]"#,
                ),
                "/api/v2/torrents/info" => reply(
                    200,
                    r#"[
                    {"hash":"aa","name":"Downloading","category":"tv","size":1000,"total_size":1000,"amount_left":400,"state":"downloading","content_path":"/dl/Downloading","ratio":0.0,"eta":120,"dlspeed":50},
                    {"hash":"bb","name":"Seeding","category":"tv","size":1000,"total_size":1000,"amount_left":0,"state":"stalledUP","content_path":"/dl/Seeding","ratio":0.4,"eta":8640000,"dlspeed":0},
                    {"hash":"cc","name":"Done","category":"tv","size":1000,"total_size":1000,"amount_left":0,"state":"stoppedUP","content_path":"/dl/Done","ratio":2.0,"eta":0,"dlspeed":0},
                    {"hash":"dd","name":"Broken","category":"tv","size":1000,"total_size":1000,"amount_left":10,"state":"missingFiles","content_path":"/dl/Broken","ratio":0.0,"eta":0,"dlspeed":0}]"#,
                ),
                "/api/v2/torrents/pause" => reply(404, ""), // v5 renamed it
                "/api/v2/torrents/stop" | "/api/v2/torrents/delete" | "/api/v2/torrents/resume" => {
                    reply(200, "")
                }
                _ => reply(404, ""),
            }
        }),
    );
    let settings =
        json!({"server": "http://qb.lan:8080", "username": "admin", "password": "secret"})
            .to_string();
    plugin.test(&settings).await.unwrap();
    let wrong = json!({"server": "http://qb.lan:8080", "username": "admin", "password": "nope"})
        .to_string();
    assert!(
        plugin
            .test(&wrong)
            .await
            .unwrap_err()
            .to_string()
            .contains("refused")
    );

    // A magnet carries its own hash; a torrent link is found again by its tag.
    let magnet = format!(
        "magnet:?xt=urn:btih:{}&dn=x",
        "0123456789abcdef0123456789abcdef01234567"
    );
    assert_eq!(
        plugin
            .add(&add(&magnet, Some("tv"), None), &settings)
            .await
            .unwrap(),
        "0123456789ABCDEF0123456789ABCDEF01234567"
    );
    assert_eq!(
        plugin
            .add(
                &add("https://idx/a.torrent", Some("tv"), Some("ABCD")),
                &settings
            )
            .await
            .unwrap(),
        "ABCD"
    );
    assert_eq!(
        plugin
            .add(&add("https://idx/a.torrent", None, None), &settings)
            .await
            .unwrap(),
        "AABBCCDDEEFF00112233445566778899AABBCCDD"
    );
    let adds: Vec<_> = seen
        .seen
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r.url.ends_with("/torrents/add"))
        .cloned()
        .collect();
    assert!(
        adds[0].body.as_deref().unwrap().contains("category=tv")
            && adds[0].body.as_deref().unwrap().contains("urls=magnet")
    );

    let items = plugin.items(&settings).await.unwrap();
    let by = |id: &str| items.iter().find(|i| i.id == id.to_uppercase()).unwrap();
    assert_eq!(
        (
            by("aa").status,
            by("aa").remaining_size,
            by("aa").eta_seconds,
            by("aa").download_speed
        ),
        (ClientStatus::Downloading, 400, Some(120), 50)
    );
    assert_eq!(
        (
            by("bb").status,
            by("bb").can_be_removed,
            by("bb").can_move_files
        ),
        (ClientStatus::Completed, false, false),
        "still seeding"
    );
    assert_eq!(
        (
            by("cc").status,
            by("cc").can_be_removed,
            by("cc").output_path.as_deref()
        ),
        (ClientStatus::Completed, true, Some("/dl/Done"))
    );
    assert_eq!(by("dd").status, ClientStatus::Failed);

    plugin.pause("AA", &settings).await.unwrap();
    plugin.resume("AA", &settings).await.unwrap();
    plugin.remove("AA", true, &settings).await.unwrap();
    let delete = seen
        .seen
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|r| r.url.ends_with("/torrents/delete"))
        .cloned()
        .unwrap();
    let body = delete.body.unwrap();
    assert!(
        body.contains("hashes=aa") && body.contains("deleteFiles=true"),
        "{body}"
    );
}

#[tokio::test]
async fn transmission_negotiates_the_session_id() {
    let (plugin, seen, _) = load(
        "transmission",
        Box::new(|r| {
            if !r
                .headers
                .iter()
                .any(|(k, v)| k == "x-transmission-session-id" && v == "SESSION1")
            {
                return HttpResponse {
                    status: 409,
                    body: "".into(),
                    headers: vec![("x-transmission-session-id".into(), "SESSION1".into())],
                };
            }
            let v: Value = serde_json::from_str(r.body.as_deref().unwrap()).unwrap();
            match v["method"].as_str().unwrap() {
                "session-get" => reply(200, r#"{"result":"success","arguments":{}}"#),
                "torrent-add" => reply(
                    200,
                    r#"{"result":"success","arguments":{"torrent-added":{"hashString":"abcdef"}}}"#,
                ),
                "torrent-get" => reply(
                    200,
                    r#"{"result":"success","arguments":{"torrents":[
                    {"hashString":"aa","name":"Going","totalSize":1000,"leftUntilDone":500,"metadataPercentComplete":1,"status":4,"downloadDir":"/dl/","error":0,"errorString":"","uploadRatio":0.1,"eta":50,"rateDownload":10,"labels":["tv"]},
                    {"hashString":"bb","name":"Finished","totalSize":1000,"leftUntilDone":0,"status":0,"downloadDir":"/dl","error":0,"errorString":"","uploadRatio":2.0,"eta":-1,"rateDownload":0,"labels":[]},
                    {"hashString":"cc","name":"Seeding","totalSize":1000,"leftUntilDone":0,"status":6,"downloadDir":"/dl","error":0,"errorString":"","uploadRatio":0.5,"eta":-1,"rateDownload":0,"labels":[]},
                    {"hashString":"ee","name":"Magnet","totalSize":0,"leftUntilDone":0,"metadataPercentComplete":0,"status":4,"downloadDir":"/dl","error":0,"errorString":"","uploadRatio":0,"eta":-1,"rateDownload":0,"labels":[]},
                    {"hashString":"dd","name":"Bad","totalSize":1000,"leftUntilDone":10,"status":0,"downloadDir":"/dl","error":3,"errorString":"disk full","uploadRatio":0,"eta":-1,"rateDownload":0,"labels":[]}]}}"#,
                ),
                "torrent-stop" | "torrent-start" | "torrent-remove" => {
                    reply(200, r#"{"result":"success","arguments":{}}"#)
                }
                _ => reply(200, r#"{"result":"method not recognized"}"#),
            }
        }),
    );
    let settings =
        json!({"server": "http://tr.lan:9091", "username": "u", "password": "p"}).to_string();
    plugin.test(&settings).await.unwrap();
    assert!(
        seen.seen
            .lock()
            .unwrap()
            .first()
            .unwrap()
            .headers
            .iter()
            .any(|(k, _)| k == "authorization")
    );
    assert_eq!(
        plugin
            .add(
                &add("magnet:?xt=urn:btih:abcdef", Some("tv"), None),
                &settings
            )
            .await
            .unwrap(),
        "ABCDEF"
    );
    let items = plugin.items(&settings).await.unwrap();
    let by = |id: &str| items.iter().find(|i| i.id == id).unwrap();
    assert_eq!(
        (
            by("AA").status,
            by("AA").category.as_deref(),
            by("AA").output_path.as_deref()
        ),
        (ClientStatus::Downloading, Some("tv"), Some("/dl/Going"))
    );
    assert_eq!(
        (by("BB").status, by("BB").can_be_removed),
        (ClientStatus::Completed, true)
    );
    assert_eq!(
        (by("CC").status, by("CC").can_be_removed),
        (ClientStatus::Completed, false)
    );
    assert_eq!(
        (by("EE").status, by("EE").can_be_removed),
        (ClientStatus::Downloading, false),
        "a magnet without metadata is not finished"
    );
    assert_eq!(
        (by("DD").status, by("DD").message.as_deref()),
        (ClientStatus::Failed, Some("disk full"))
    );
    plugin.pause("AA", &settings).await.unwrap();
    plugin.resume("AA", &settings).await.unwrap();
    plugin.remove("AA", true, &settings).await.unwrap();
    let last = seen
        .seen
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .body
        .clone()
        .unwrap();
    assert!(
        last.contains("torrent-remove") && last.contains("\"delete-local-data\":true"),
        "{last}"
    );
}

#[tokio::test]
async fn nzbget_uses_json_rpc() {
    let (plugin, seen, _) = load(
        "nzbget",
        Box::new(|r| {
            let v: Value = serde_json::from_str(r.body.as_deref().unwrap()).unwrap();
            let result = match v["method"].as_str().unwrap() {
                "version" => json!("21.1"),
                "append" => json!(42),
                "listgroups" => {
                    json!([{"NZBID": 42, "NZBName": "Active", "Category": "tv", "FileSizeMB": 100, "RemainingSizeMB": 40, "Status": "DOWNLOADING"}])
                }
                "history" => json!([
                    {"NZBID": 7, "Name": "Good", "Category": "tv", "Status": "SUCCESS/ALL", "FileSizeMB": 10, "FinalDir": "/dl/Good"},
                    {"NZBID": 8, "Name": "Bad", "Category": "tv", "Status": "FAILURE/PAR", "FileSizeMB": 10, "DestDir": "/dl/Bad"}]),
                "editqueue" => json!(true),
                _ => json!(null),
            };
            reply(
                200,
                json!({"version": "1.1", "id": 1, "result": result}).to_string(),
            )
        }),
    );
    let settings =
        json!({"server": "http://nzbget.lan:6789", "username": "nzbget", "password": "x"})
            .to_string();
    plugin.test(&settings).await.unwrap();
    assert_eq!(
        plugin
            .add(&add("https://idx/get.nzb", Some("tv"), None), &settings)
            .await
            .unwrap(),
        "42"
    );
    let append: Value = serde_json::from_str(
        seen.seen
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|r| r.body.as_deref().unwrap().contains("\"append\""))
            .unwrap()
            .body
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(append["params"][0], "Some.Show.S01E01.1080p.nzb");
    assert_eq!(append["params"][1], "https://idx/get.nzb");
    assert_eq!(append["params"][2], "tv");

    let items = plugin.items(&settings).await.unwrap();
    let by = |id: &str| items.iter().find(|i| i.id == id).unwrap();
    assert_eq!(
        (by("42").status, by("42").remaining_size),
        (ClientStatus::Downloading, 40 * 1048576)
    );
    assert_eq!(
        (
            by("7").status,
            by("7").output_path.as_deref(),
            by("7").can_be_removed
        ),
        (ClientStatus::Completed, Some("/dl/Good"), true)
    );
    assert_eq!(
        (by("8").status, by("8").output_path.as_deref()),
        (ClientStatus::Failed, Some("/dl/Bad"))
    );
    plugin.pause("42", &settings).await.unwrap();
    plugin.resume("42", &settings).await.unwrap();
    plugin.remove("7", true, &settings).await.unwrap();
    let last = seen
        .seen
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .body
        .clone()
        .unwrap();
    assert!(last.contains("HistoryFinalDelete"), "{last}");
}
