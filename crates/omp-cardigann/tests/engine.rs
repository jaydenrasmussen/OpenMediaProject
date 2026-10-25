//! The engine against scripted sites: public and private, HTML, JSON and XML.

use std::sync::{Arc, Mutex};

use omp_cardigann::{
    Config, Fetched, Http, HttpRequest, HttpResponse, Indexer, Method, Query, QueryKind,
    def::Definition,
};
use serde_json::json;

type Handler = dyn Fn(&HttpRequest, &[HttpRequest]) -> HttpResponse + Send + Sync;

struct Site {
    seen: Mutex<Vec<HttpRequest>>,
    handler: Box<Handler>,
}

impl Site {
    fn new(
        handler: impl Fn(&HttpRequest, &[HttpRequest]) -> HttpResponse + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::new(vec![]),
            handler: Box::new(handler),
        })
    }
    fn requests(&self) -> Vec<HttpRequest> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl Http for Site {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, String> {
        let mut seen = self.seen.lock().unwrap();
        let response = (self.handler)(&request, &seen);
        seen.push(request);
        Ok(response)
    }
}

fn ok(body: &str) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
        body: body.as_bytes().to_vec(),
    }
}

fn with_type(mut r: HttpResponse, ty: &str) -> HttpResponse {
    r.headers = vec![("content-type".into(), ty.into())];
    r
}

fn redirect(to: &str) -> HttpResponse {
    HttpResponse {
        status: 302,
        headers: vec![("location".into(), to.into())],
        body: vec![],
    }
}

fn indexer(yaml: &str, config: Config, site: Arc<Site>) -> Indexer {
    Indexer::new(Arc::new(Definition::from_yaml(yaml).unwrap()), config, site).unwrap()
}

fn header<'a>(r: &'a HttpRequest, name: &str) -> Option<&'a str> {
    r.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

const PUBLIC: &str = r#"
id: demo
name: Demo
language: en-US
type: public
links: ["https://demo.test/"]
caps:
  categorymappings:
    - {id: 1, cat: Movies/HD, desc: "Movies HD"}
    - {id: 2, cat: TV/HD, desc: "TV HD"}
  modes:
    search: [q]
    tv-search: [q, season, ep]
    movie-search: [q]
settings:
  - name: sort
    type: select
    label: Sort
    default: seeders
    options: {seeders: Seeders, date: Date}
search:
  paths:
    - path: /search/{{ .Keywords }}/
  inputs:
    sort: "{{ .Config.sort }}"
    cat: "{{ range .Categories }}{{.}},{{end}}"
  rows:
    selector: table.results tr:has(a.name)
  fields:
    category:
      selector: td.cat
      attribute: data-id
    title:
      selector: a.name
    details:
      selector: a.name
      attribute: href
    download:
      selector: a.dl
      attribute: href
    size:
      selector: td.size
    seeders:
      selector: td.seed
    leechers:
      selector: td.leech
    date:
      selector: td.date
      filters:
        - name: dateparse
          args: "yyyy-MM-dd HH:mm"
"#;

const PAGE: &str = r#"<html><body><table class="results">
<tr><th>Name</th></tr>
<tr><td class="cat" data-id="1"></td><td><a class="name" href="/t/1">The.Matrix.1999.1080p.BluRay</a> <a class="dl" href="/dl/1.torrent">get</a></td>
<td class="size">1.5 GB</td><td class="seed">120</td><td class="leech">3</td><td class="date">2020-05-06 07:08</td></tr>
<tr><td class="cat" data-id="2"></td><td><a class="name" href="/t/2">Show.S01E02.720p</a> <a class="dl" href="magnet:?xt=urn:btih:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa">m</a></td>
<td class="size">700 MB</td><td class="seed">1,234</td><td class="leech">-</td><td class="date">2021-01-02 03:04</td></tr>
</table></body></html>"#;

#[tokio::test]
async fn a_public_html_site_is_searched_and_read() {
    let site = Site::new(|_, _| ok(PAGE));
    let idx = indexer(PUBLIC, Config::new(), site.clone());
    let q = Query {
        q: "The Matrix".into(),
        year: Some(1999),
        categories: vec![2000],
        ..Default::default()
    };
    let found = idx.search(&q).await.unwrap();

    let req = &site.requests()[0];
    assert_eq!(req.method, Method::Get);
    // The keywords are in the path, encoded; the mapped category and the default sort are in the query.
    assert_eq!(
        req.url,
        "https://demo.test/search/The%20Matrix%201999/?sort=seeders&cat=1%2C"
    );

    assert_eq!(found.len(), 2);
    let m = &found[0];
    assert_eq!(m.title, "The.Matrix.1999.1080p.BluRay");
    assert_eq!(m.details.as_deref(), Some("https://demo.test/t/1"));
    assert_eq!(
        m.download_url.as_deref(),
        Some("https://demo.test/dl/1.torrent")
    );
    assert_eq!(m.size, 1_610_612_736);
    assert_eq!((m.seeders, m.peers), (Some(120), Some(123)));
    assert_eq!(
        m.publish_date.unwrap().to_rfc3339(),
        "2020-05-06T07:08:00+00:00"
    );
    assert_eq!(m.categories, vec![2040, 100_001]);

    let s = &found[1];
    assert_eq!(s.seeders, Some(1234));
    assert!(
        s.magnet
            .as_deref()
            .unwrap()
            .starts_with("magnet:?xt=urn:btih:aaaa")
    );
    assert_eq!(
        s.info_hash.as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
    assert_eq!(s.categories, vec![5040, 100_002]);
}

#[tokio::test]
async fn settings_choose_what_is_sent() {
    let site = Site::new(|_, _| ok(PAGE));
    let mut cfg = Config::new();
    cfg.insert("sort".into(), json!("date"));
    let idx = indexer(PUBLIC, cfg, site.clone());
    let q = Query {
        kind: QueryKind::Tv,
        q: "Show".into(),
        season: Some(1),
        episode: Some("S01E02".into()),
        categories: vec![5000],
        ..Default::default()
    };
    idx.search(&q).await.unwrap();
    assert_eq!(
        site.requests()[0].url,
        "https://demo.test/search/Show%20S01E02/?sort=date&cat=2%2C"
    );
}

#[tokio::test]
async fn a_failing_site_is_an_error_and_a_blocked_one_says_why() {
    let down = Site::new(|_, _| HttpResponse {
        status: 500,
        ..Default::default()
    });
    let err = indexer(PUBLIC, Config::new(), down)
        .search(&Query::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("500"), "{err}");

    let cf = Site::new(|_, _| HttpResponse {
        status: 403,
        headers: vec![("server".into(), "cloudflare".into())],
        body: b"<title>Just a moment...</title>".to_vec(),
    });
    let err = indexer(PUBLIC, Config::new(), cf)
        .search(&Query::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Cloudflare"), "{err}");
}

const JSON_SITE: &str = r#"
id: jsonsite
name: Json Site
language: en-US
type: public
links: ["https://api.test/"]
caps:
  categorymappings:
    - {id: movies, cat: Movies, desc: Movies}
  modes: {search: [q]}
search:
  paths:
    - path: api/list
      response: {type: json}
  inputs:
    q: "{{ .Keywords }}"
    limit: "50"
  rows:
    selector: data.items
    count: {selector: data.total}
  fields:
    title: {selector: name}
    download: {selector: "links.torrent"}
    infohash: {selector: hash}
    size: {selector: bytes}
    seeders: {selector: seeds}
    leechers: {selector: peers}
    date: {selector: added}
    category: {text: movies}
    imdb: {selector: imdb, optional: true}
"#;

#[tokio::test]
async fn a_json_site_is_read() {
    let body = json!({"data": {"total": 2, "items": [
        {"name": "Alpha 2020", "links": {"torrent": "/get/1"}, "hash": "0123456789abcdef0123456789abcdef01234567", "bytes": 1048576, "seeds": 5, "peers": 2, "added": 1700000000, "imdb": "tt0133093"},
        {"name": "Beta 2021", "links": {"torrent": "/get/2"}, "hash": "", "bytes": 2048, "seeds": 0, "peers": 0, "added": "1700000100"}
    ]}});
    let site = Site::new(move |_, _| with_type(ok(&body.to_string()), "application/json"));
    let idx = indexer(JSON_SITE, Config::new(), site.clone());
    let found = idx
        .search(&Query {
            q: "alpha".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        site.requests()[0].url,
        "https://api.test/api/list?q=alpha&limit=50"
    );
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].title, "Alpha 2020");
    assert_eq!(
        found[0].download_url.as_deref(),
        Some("https://api.test/get/1")
    );
    assert_eq!(found[0].size, 1_048_576);
    assert_eq!(found[0].imdb_id, Some(133093));
    assert_eq!(found[0].publish_date.unwrap().timestamp(), 1_700_000_000);
    assert_eq!(found[0].categories[0], 2000);
    assert!(
        found[0].categories[1] >= 100_000,
        "the site's own category gets a number of its own"
    );
    // A public site's hash becomes a magnet link.
    assert!(
        found[0]
            .magnet
            .as_deref()
            .unwrap()
            .contains("urn:btih:0123456789abcdef")
    );
    assert_eq!(found[1].imdb_id, None);
}

#[tokio::test]
async fn an_empty_json_result_is_no_results() {
    let site =
        Site::new(|_, _| with_type(ok(r#"{"data":{"total":0,"items":[]}}"#), "application/json"));
    let found = indexer(JSON_SITE, Config::new(), site)
        .search(&Query::default())
        .await
        .unwrap();
    assert!(found.is_empty());
}

const XML_SITE: &str = r#"
id: feed
name: Feed
language: en-US
type: public
links: ["https://feed.test/"]
caps:
  categorymappings: [{id: 1, cat: TV/Anime}]
  modes: {search: [q]}
search:
  paths:
    - path: rss.xml
      response: {type: xml}
  rows:
    selector: rss > channel > item
  fields:
    title: {selector: title}
    details: {selector: link}
    download: {selector: enclosure, attribute: url}
    size: {selector: enclosure, attribute: length}
    date: {selector: pubDate}
    category: {text: 1}
    seeders: {text: 1}
"#;

#[tokio::test]
async fn an_rss_feed_is_read() {
    let xml = r#"<?xml version="1.0"?><rss><channel>
      <item><title>[Group] Show - 01</title><link>https://feed.test/v/1</link>
      <enclosure url="https://feed.test/f/1.torrent" length="12345" type="application/x-bittorrent"/>
      <pubDate>Tue, 05 Mar 2024 10:00:00 +0000</pubDate></item></channel></rss>"#;
    let site = Site::new(move |_, _| with_type(ok(xml), "application/rss+xml"));
    let found = indexer(XML_SITE, Config::new(), site)
        .search(&Query::default())
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "[Group] Show - 01");
    assert_eq!(found[0].size, 12345);
    assert_eq!(
        found[0].download_url.as_deref(),
        Some("https://feed.test/f/1.torrent")
    );
    assert_eq!(
        found[0].publish_date.unwrap().to_rfc3339(),
        "2024-03-05T10:00:00+00:00"
    );
}

const PRIVATE: &str = r#"
id: priv
name: Private
language: en-US
type: private
links: ["https://priv.test/"]
caps:
  categorymappings: [{id: 1, cat: Movies}]
  modes: {search: [q]}
login:
  path: login.php
  method: form
  form: form#login
  inputs:
    username: "{{ .Config.username }}"
    password: "{{ .Config.password }}"
  error:
    - selector: div.error
  test:
    path: index.php
    selector: a.logout
search:
  paths:
    - path: browse.php
  inputs:
    q: "{{ .Keywords }}"
  rows:
    selector: tr.t
  fields:
    title: {selector: a.n}
    download: {selector: a.n, attribute: href}
    size: {text: 10 MB}
    seeders: {text: 4}
download:
  selectors:
    - selector: a.dlbtn
      attribute: href
"#;

fn private_site(good_password: &'static str) -> Arc<Site> {
    Site::new(move |req, _| {
        let cookie = header(req, "cookie").unwrap_or_default().to_owned();
        let signed_in = cookie.contains("session=abc");
        let path = req
            .url
            .split('?')
            .next()
            .unwrap()
            .trim_start_matches("https://priv.test")
            .to_owned();
        match (req.method, path.as_str()) {
            (Method::Get, "/login.php") => ok(
                r#"<form id="login" action="takelogin.php" method="post"><input type="hidden" name="token" value="T0K"><input name="username"><input name="password" type="password"><input type="checkbox" name="remember" value="1"></form>"#,
            ),
            (Method::Post, "/takelogin.php") => {
                let body = String::from_utf8(req.body.clone().unwrap().1).unwrap();
                if body.contains(&format!("password={good_password}"))
                    && body.contains("token=T0K")
                    && body.contains("username=sam")
                {
                    let mut r = redirect("/index.php");
                    r.headers
                        .push(("set-cookie".into(), "session=abc; Path=/; HttpOnly".into()));
                    r
                } else {
                    ok(r#"<div class="error">Wrong password</div>"#)
                }
            }
            (Method::Get, "/index.php") if signed_in => ok(r#"<a class="logout">out</a>"#),
            (Method::Get, "/browse.php") if signed_in => ok(
                r#"<table><tr class="t"><td><a class="n" href="/details.php?id=9">Private.Movie.2020</a></td></tr></table>"#,
            ),
            (Method::Get, "/details.php") if signed_in => {
                ok(r#"<a class="dlbtn" href="/download.php?id=9">Download</a>"#)
            }
            (Method::Get, "/download.php") if signed_in => HttpResponse {
                status: 200,
                headers: vec![],
                body: b"d8:announce3:urle".to_vec(),
            },
            (Method::Get, _) => redirect("/login.php"),
            _ => HttpResponse {
                status: 404,
                ..Default::default()
            },
        }
    })
}

fn creds(password: &str) -> Config {
    let mut c = Config::new();
    c.insert("username".into(), json!("sam"));
    c.insert("password".into(), json!(password));
    c
}

#[tokio::test]
async fn a_private_site_is_signed_into_with_the_form_it_shows() {
    let site = private_site("secret");
    let idx = indexer(PRIVATE, creds("secret"), site.clone());
    let found = idx
        .search(&Query {
            q: "private".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "Private.Movie.2020");
    // The hidden token and the ticked box came from the page; the name and password from the settings.
    let post = site
        .requests()
        .into_iter()
        .find(|r| r.method == Method::Post)
        .unwrap();
    let body = String::from_utf8(post.body.unwrap().1).unwrap();
    assert!(
        body.contains("token=T0K")
            && body.contains("username=sam")
            && body.contains("password=secret"),
        "{body}"
    );
    assert!(!body.contains("remember"), "an unticked box is not sent");
    // The session cookie went along with the search.
    let search = site
        .requests()
        .into_iter()
        .find(|r| r.url.contains("browse.php"))
        .unwrap();
    assert_eq!(header(&search, "cookie"), Some("session=abc"));
}

#[tokio::test]
async fn a_wrong_password_is_reported_with_the_sites_words() {
    let idx = indexer(PRIVATE, creds("nope"), private_site("secret"));
    let err = idx.search(&Query::default()).await.unwrap_err();
    assert!(err.to_string().contains("Wrong password"), "{err}");
}

#[tokio::test]
async fn the_torrent_behind_a_details_page_is_found_after_signing_in() {
    let idx = indexer(PRIVATE, creds("secret"), private_site("secret"));
    let got = idx
        .download("https://priv.test/details.php?id=9")
        .await
        .unwrap();
    assert_eq!(got, Fetched::Torrent(b"d8:announce3:urle".to_vec()));
}

#[tokio::test]
async fn testing_checks_the_sign_in_and_a_search() {
    indexer(PRIVATE, creds("secret"), private_site("secret"))
        .test()
        .await
        .unwrap();
    assert!(
        indexer(PRIVATE, creds("nope"), private_site("secret"))
            .test()
            .await
            .is_err()
    );
}

const COOKIE_SITE: &str = r#"
id: ck
name: Cookie
language: en-US
type: private
links: ["https://ck.test/"]
caps: {categorymappings: [{id: 1, cat: Movies}], modes: {search: [q]}}
settings:
  - {name: cookie, type: text, label: Cookie}
login:
  method: cookie
  test: {path: /, selector: a.me}
search:
  paths: [{path: /s}]
  rows: {selector: tr.r}
  fields:
    title: {selector: td}
    download: {text: "/d/1"}
"#;

#[tokio::test]
async fn a_pasted_cookie_is_the_session() {
    let site = Site::new(|req, _| {
        let cookie = header(req, "cookie").unwrap_or_default();
        if cookie.contains("uid=7") && cookie.contains("pass=x") {
            ok(
                r#"<a class="me">me</a><table><tr class="r"><td>Cookie.Movie.2020</td></tr></table>"#,
            )
        } else {
            redirect("/login")
        }
    });
    let mut cfg = Config::new();
    cfg.insert("cookie".into(), json!("uid=7; pass=x"));
    let found = indexer(COOKIE_SITE, cfg, site)
        .search(&Query::default())
        .await
        .unwrap();
    assert_eq!(found[0].title, "Cookie.Movie.2020");

    let site = Site::new(|_, _| redirect("/login"));
    assert!(
        indexer(COOKIE_SITE, Config::new(), site)
            .search(&Query::default())
            .await
            .is_err()
    );
}
