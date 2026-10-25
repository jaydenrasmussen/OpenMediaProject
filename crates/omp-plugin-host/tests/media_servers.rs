//! The Plex and Jellyfin library plugins, built from source and run in the real
//! host against scripted stand-ins for each server's API.

use std::{
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use omp_plugin_host::{
    Fetcher, HttpRequest, HttpResponse, Manifest, MediaItemKind, MediaServer, Permissions, Plugin,
    PluginHost,
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

fn load(name: &str, script: Script) -> (Plugin<MediaServer>, Arc<Scripted>, Manifest) {
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
        .load_media_server(
            &wasm,
            Permissions {
                allowed_hosts: manifest.allowed_hosts.clone(),
            },
        )
        .unwrap();
    (plugin, fetcher, manifest)
}

fn header<'a>(r: &'a HttpRequest, name: &str) -> Option<&'a str> {
    r.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// The part of a URL after the host.
fn path_and_query(r: &HttpRequest) -> String {
    let u = url::Url::parse(&r.url).unwrap();
    match u.query() {
        Some(q) => format!("{}?{q}", u.path()),
        None => u.path().to_owned(),
    }
}

fn q(r: &HttpRequest, key: &str) -> Option<String> {
    url::Url::parse(&r.url)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

const JELLYFIN: &str = r#"{"url":"http://jf:8096/","api_key":"KEY"}"#;

fn jellyfin_server() -> Script {
    Box::new(|r| {
        if header(r, "x-emby-token") != Some("KEY") {
            return reply(401, "");
        }
        let path = path_and_query(r);
        if path.starts_with("/System/Info") {
            return reply(200, r#"{"ServerName":"Den","Version":"10.10.3"}"#);
        }
        // Three items, served two at a time.
        let start: usize = q(r, "StartIndex").and_then(|s| s.parse().ok()).unwrap_or(0);
        let all = [
            r#"{"Id":"m1","Type":"Movie","Name":"The Matrix","ProductionYear":1999,"ProviderIds":{"Tmdb":"603","Imdb":"tt0133093"}}"#,
            r#"{"Id":"s1","Type":"Series","Name":"Breaking Bad","ProductionYear":2008,"ProviderIds":{"Tvdb":"81189","Tmdb":"1396"}}"#,
            r#"{"Id":"e1","Type":"Episode","Name":"Pilot","SeriesName":"Breaking Bad","SeriesId":"s1","ParentIndexNumber":1,"IndexNumber":1,"ProviderIds":{}}"#,
        ];
        let page: Vec<&str> = all.iter().skip(start).take(2).copied().collect();
        assert_eq!(q(r, "Limit").as_deref(), Some("200"));
        reply(
            200,
            format!(
                r#"{{"Items":[{}],"TotalRecordCount":3,"StartIndex":{start}}}"#,
                page.join(",")
            ),
        )
    })
}

#[tokio::test]
async fn jellyfin_reads_its_whole_library_page_by_page() {
    let (plugin, seen, _) = load("jellyfin-library", jellyfin_server());
    let first = plugin.library_page(None, JELLYFIN).await.unwrap();
    assert_eq!(first.items.len(), 2);
    assert_eq!(first.next.as_deref(), Some("2"));
    let m = &first.items[0];
    assert_eq!(
        (m.kind, m.title.as_str(), m.year),
        (MediaItemKind::Movie, "The Matrix", Some(1999))
    );
    assert_eq!(
        (m.tmdb_id, m.imdb_id.as_deref()),
        (Some(603), Some("tt0133093"))
    );
    assert_eq!(first.items[1].tvdb_id, Some(81189));

    let second = plugin
        .library_page(first.next.as_deref(), JELLYFIN)
        .await
        .unwrap();
    assert_eq!(second.next, None, "the last page says so");
    let e = &second.items[0];
    assert_eq!(e.kind, MediaItemKind::Episode);
    assert_eq!(
        (e.series_key.as_deref(), e.season, e.episode),
        (Some("s1"), Some(1), Some(1))
    );
    assert_eq!(
        e.title, "Breaking Bad",
        "an episode carries its series' name"
    );

    // The key goes in a header, never the URL.
    let seen = seen.seen.lock().unwrap();
    assert!(seen.iter().all(|r| !r.url.contains("KEY")));
}

#[tokio::test]
async fn jellyfin_tests_the_connection_and_explains_failures() {
    let (plugin, _, _) = load("jellyfin-library", jellyfin_server());
    assert_eq!(plugin.test(JELLYFIN).await.unwrap(), "Den 10.10.3");
    let bad = plugin
        .test(r#"{"url":"http://jf:8096","api_key":"WRONG"}"#)
        .await
        .unwrap_err();
    assert!(bad.to_string().contains("refused the API key"), "{bad}");
    let empty = plugin
        .test(r#"{"url":"","api_key":"x"}"#)
        .await
        .unwrap_err();
    assert!(empty.to_string().contains("address is empty"), "{empty}");
}

const PLEX: &str = r#"{"url":"http://plex:32400","token":"TOK"}"#;

fn plex_server() -> Script {
    Box::new(|r| {
        if header(r, "x-plex-token") != Some("TOK") {
            return reply(401, "");
        }
        let u = url::Url::parse(&r.url).unwrap();
        let start: usize = header(r, "x-plex-container-start")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        match u.path() {
            "/" => reply(
                200,
                r#"{"MediaContainer":{"friendlyName":"Plex at home","version":"1.41"}}"#,
            ),
            "/library/sections" => reply(
                200,
                r#"{"MediaContainer":{"Directory":[{"key":"1","type":"movie"},{"key":"2","type":"show"},{"key":"3","type":"artist"}]}}"#,
            ),
            "/library/sections/1/all" => {
                assert_eq!(q(r, "type").as_deref(), Some("1"));
                assert_eq!(q(r, "includeGuids").as_deref(), Some("1"));
                reply(
                    200,
                    r#"{"MediaContainer":{"totalSize":1,"Metadata":[{"ratingKey":"11","title":"The Matrix","year":1999,"Guid":[{"id":"imdb://tt0133093"},{"id":"tmdb://603"},{"id":"tvdb://169"}]}]}}"#,
                )
            }
            "/library/sections/2/all" if q(r, "type").as_deref() == Some("2") => reply(
                200,
                r#"{"MediaContainer":{"totalSize":1,"Metadata":[{"ratingKey":"20","title":"Breaking Bad","year":2008,"Guid":[{"id":"tvdb://81189"}]}]}}"#,
            ),
            "/library/sections/2/all" => {
                // Episodes, one at a time, to exercise the paging inside a section.
                let eps = [
                    r#"{"ratingKey":"21","grandparentRatingKey":"20","grandparentTitle":"Breaking Bad","parentIndex":1,"index":1}"#,
                    r#"{"ratingKey":"22","grandparentRatingKey":"20","grandparentTitle":"Breaking Bad","parentIndex":1,"index":2}"#,
                ];
                let page: Vec<&str> = eps.iter().skip(start).take(1).copied().collect();
                reply(
                    200,
                    format!(
                        r#"{{"MediaContainer":{{"totalSize":2,"Metadata":[{}]}}}}"#,
                        page.join(",")
                    ),
                )
            }
            other => reply(404, other),
        }
    })
}

#[tokio::test]
async fn plex_walks_movie_sections_then_shows_then_episodes() {
    let (plugin, _, _) = load("plex-library", plex_server());
    let mut items = Vec::new();
    let mut cursor: Option<String> = None;
    let mut cursors = Vec::new();
    loop {
        let page = plugin.library_page(cursor.as_deref(), PLEX).await.unwrap();
        items.extend(page.items);
        match page.next {
            Some(n) => {
                cursors.push(n.clone());
                cursor = Some(n);
            }
            None => break,
        }
        assert!(cursors.len() < 10, "the walk must end: {cursors:?}");
    }
    // Section 3 (music) is skipped; the episodes took two pages.
    assert_eq!(cursors, ["2.2.0", "2.4.0", "2.4.1"]);
    let kinds: Vec<_> = items.iter().map(|i| (i.kind, i.key.as_str())).collect();
    assert_eq!(
        kinds,
        [
            (MediaItemKind::Movie, "11"),
            (MediaItemKind::Series, "20"),
            (MediaItemKind::Episode, "21"),
            (MediaItemKind::Episode, "22"),
        ]
    );
    let m = &items[0];
    assert_eq!(
        (m.tmdb_id, m.imdb_id.as_deref(), m.tvdb_id),
        (Some(603), Some("tt0133093"), Some(169))
    );
    let e = &items[3];
    assert_eq!(
        (e.series_key.as_deref(), e.season, e.episode),
        (Some("20"), Some(1), Some(2))
    );
    assert_eq!(e.title, "Breaking Bad");
}

#[tokio::test]
async fn plex_tests_the_connection_and_explains_failures() {
    let (plugin, _, _) = load("plex-library", plex_server());
    assert_eq!(plugin.test(PLEX).await.unwrap(), "Plex at home 1.41");
    let bad = plugin
        .test(r#"{"url":"http://plex:32400","token":"NOPE"}"#)
        .await
        .unwrap_err();
    assert!(bad.to_string().contains("refused the token"), "{bad}");
}

#[tokio::test]
async fn both_plugins_describe_their_settings() {
    for (name, field) in [("jellyfin-library", "api_key"), ("plex-library", "token")] {
        let (plugin, _, manifest) = load(name, Box::new(|_| reply(200, "{}")));
        let schema: Value = serde_json::from_str(&plugin.settings_schema().await.unwrap()).unwrap();
        assert_eq!(
            schema["properties"][field]["format"],
            json!("password"),
            "{name}"
        );
        assert_eq!(manifest.allowed_hosts, ["*"]);
    }
}
