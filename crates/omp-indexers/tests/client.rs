use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Router,
    extract::{Query, State},
    http::{HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use omp_core::release::DownloadProtocol;
use omp_indexers::{Flavor, IndexerClient, IndexerError, IndexerSettings, LastSeen, MovieSearch};

mod common;
use common::identity;

type Params = HashMap<String, String>;

struct Reply {
    status: u16,
    content_type: &'static str,
    body: String,
    headers: Vec<(&'static str, String)>,
}

impl Reply {
    fn xml(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            content_type: "application/rss+xml",
            body: body.into(),
            headers: vec![],
        }
    }
}

struct Mock {
    base: String,
    log: Arc<Mutex<Vec<Params>>>,
}

impl Mock {
    fn requests(&self) -> Vec<Params> {
        self.log.lock().unwrap().clone()
    }

    fn of_type(&self, t: &str) -> Vec<Params> {
        self.requests()
            .into_iter()
            .filter(|p| p.get("t").map(String::as_str) == Some(t))
            .collect()
    }
}

type Handler = Arc<dyn Fn(&Params) -> Reply + Send + Sync>;

async fn spawn(handler: impl Fn(&Params) -> Reply + Send + Sync + 'static) -> Mock {
    let log = Arc::new(Mutex::new(Vec::new()));
    let state = (Arc::new(handler) as Handler, log.clone());
    async fn serve(
        State((handler, log)): State<(Handler, Arc<Mutex<Vec<Params>>>)>,
        Query(params): Query<Params>,
    ) -> Response {
        log.lock().unwrap().push(params.clone());
        let reply = handler(&params);
        let mut response = (
            StatusCode::from_u16(reply.status).unwrap(),
            [(axum::http::header::CONTENT_TYPE, reply.content_type)],
            reply.body,
        )
            .into_response();
        for (k, v) in reply.headers {
            response.headers_mut().insert(
                HeaderName::from_static(k),
                HeaderValue::from_str(&v).unwrap(),
            );
        }
        response
    }
    let app = Router::new().route("/api", get(serve)).with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Mock { base, log }
}

fn client(base: &str, flavor: Flavor) -> IndexerClient {
    let settings = IndexerSettings {
        base_url: base.to_owned(),
        api_key: "KEY".into(),
        categories: vec![2000],
        ..Default::default()
    };
    IndexerClient::new(flavor, settings, identity())
        .unwrap()
        .with_rate_limit(Duration::ZERO)
}

const CAPS: &str = include_str!("data/newznab_caps.xml");

/// `n` releases starting at index `from`, newest first, one minute apart.
fn feed(from: usize, n: usize) -> String {
    let mut items = String::new();
    for i in from..from + n {
        let minutes = 600 - (i as i64 % 590);
        items.push_str(&format!(
            r#"<item><title>Movie.{i}.1080p.WEB-DL-GRP</title><guid>g{i}</guid>
            <pubDate>Fri, 16 Oct 2026 {:02}:{:02}:00 +0000</pubDate>
            <enclosure url="https://idx.example/dl/{i}.nzb" length="1000" type="application/x-nzb"/></item>"#,
            minutes / 60,
            minutes % 60
        ));
    }
    format!(
        r#"<?xml version="1.0"?><rss xmlns:newznab="http://www.newznab.com/DTD/2010/feeds/attributes/"><channel>{items}</channel></rss>"#
    )
}

fn caps_or(
    handler: impl Fn(&Params) -> Reply + Send + Sync + 'static,
) -> impl Fn(&Params) -> Reply + Send + Sync + 'static {
    move |p| {
        if p.get("t").map(String::as_str) == Some("caps") {
            Reply::xml(CAPS)
        } else {
            handler(p)
        }
    }
}

#[tokio::test]
async fn capabilities_are_fetched_once_and_requests_carry_the_key() {
    let mock = spawn(caps_or(|_| {
        Reply::xml(include_str!("data/newznab_movies.xml"))
    }))
    .await;
    let c = client(&mock.base, Flavor::Newznab);
    let first = c.fetch_recent(None).await.unwrap();
    c.fetch_recent(None).await.unwrap();

    assert_eq!(mock.of_type("caps").len(), 1, "caps cached");
    let movie = mock.of_type("movie");
    assert_eq!(movie.len(), 2);
    assert_eq!(movie[0]["apikey"], "KEY");
    assert_eq!(movie[0]["cat"], "2000");
    assert_eq!(movie[0]["extended"], "1");
    assert_eq!(movie[0]["limit"], "100");
    assert_eq!(movie[0]["offset"], "0");

    // Fourth item has no download link and is dropped; the rest are newest first.
    let titles: Vec<_> = first.releases.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(titles.len(), 3);
    assert!(titles[0].starts_with("The.Matrix.1999.720p"), "{titles:?}");
    assert!(
        first
            .releases
            .iter()
            .all(|r| r.protocol == DownloadProtocol::Usenet)
    );
    assert_eq!(
        first.newest.as_ref().unwrap().download_url,
        first.releases[0].download_url
    );
}

#[tokio::test]
async fn paging_continues_while_pages_are_full() {
    let mock = spawn(caps_or(|p| {
        let offset: usize = p["offset"].parse().unwrap();
        Reply::xml(match offset {
            0 => feed(0, 100),
            100 => feed(100, 100),
            _ => feed(200, 30),
        })
    }))
    .await;
    let c = client(&mock.base, Flavor::Newznab);
    let found = c
        .search_movie(&MovieSearch {
            tmdb_id: 603,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(found.len(), 230);
    let offsets: Vec<_> = mock
        .of_type("movie")
        .iter()
        .map(|p| p["offset"].clone())
        .collect();
    assert_eq!(offsets, ["0", "100", "200"]);
}

#[tokio::test]
async fn recent_sync_stops_at_the_last_seen_release() {
    let mock = spawn(caps_or(|p| {
        let offset: usize = p["offset"].parse().unwrap();
        Reply::xml(feed(offset, 100))
    }))
    .await;
    let c = client(&mock.base, Flavor::Newznab);

    // The previous sync saw release 150, which sits on page two.
    let last = LastSeen {
        publish_date: chrono::DateTime::parse_from_rfc3339("2026-10-16T00:00:00Z")
            .unwrap()
            .into(),
        download_url: "https://idx.example/dl/150.nzb".into(),
    };
    let synced = c.fetch_recent(Some(last)).await.unwrap();
    assert_eq!(
        mock.of_type("movie").len(),
        2,
        "stopped after the page holding the last-seen release"
    );
    assert!(synced.fully_updated);
    assert_eq!(synced.releases.len(), 200);

    // With no history, one page is enough to start from.
    let mock2 = spawn(caps_or(|p| {
        Reply::xml(feed(p["offset"].parse().unwrap(), 100))
    }))
    .await;
    let c2 = client(&mock2.base, Flavor::Newznab);
    let first_ever = c2.fetch_recent(None).await.unwrap();
    assert_eq!(mock2.of_type("movie").len(), 1);
    assert!(first_ever.fully_updated);
}

#[tokio::test]
async fn id_search_comes_first_and_text_search_only_when_it_finds_nothing() {
    let empty = r#"<rss><channel></channel></rss>"#;
    // Ids find nothing; the text query finds one.
    let mock = spawn(caps_or(move |p| {
        if p.contains_key("q") {
            Reply::xml(feed(0, 1))
        } else {
            Reply::xml(empty)
        }
    }))
    .await;
    let c = client(&mock.base, Flavor::Newznab);
    let search = MovieSearch {
        tmdb_id: 603,
        imdb_id: Some("tt0133093".into()),
        year: 1999,
        titles: vec!["The Matrix".into()],
    };
    let found = c.search_movie(&search).await.unwrap();
    assert_eq!(found.len(), 1);
    let movie = mock.of_type("movie");
    assert_eq!(movie[0]["tmdbid"], "603");
    assert_eq!(
        movie[0]["imdbid"], "0133093",
        "aggregate id search sends both"
    );
    let text = mock.of_type("search");
    assert_eq!(text.len(), 1);
    // The movie search engine is "raw" in the fixture caps, but the plain
    // search engine is "sphinx", so the title is cleaned.
    assert_eq!(text[0]["q"], "Matrix 1999");

    // When the id search succeeds the text tier is never requested.
    let mock = spawn(caps_or(|_| Reply::xml(feed(0, 2)))).await;
    let found = client(&mock.base, Flavor::Newznab)
        .search_movie(&search)
        .await
        .unwrap();
    assert_eq!(found.len(), 2);
    assert!(mock.of_type("search").is_empty());
}

#[tokio::test]
async fn duplicates_by_guid_are_removed() {
    let mock = spawn(caps_or(|_| {
        // The same item twice in one feed.
        let one = feed(5, 1);
        let start = one.find("<item>").unwrap();
        let end = one.find("</channel>").unwrap();
        let item = &one[start..end];
        Reply::xml(format!("{}{item}{item}{}", &one[..end], &one[end..]))
    }))
    .await;
    let found = client(&mock.base, Flavor::Newznab)
        .search_movie(&MovieSearch {
            tmdb_id: 1,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
}

#[tokio::test]
async fn torznab_end_to_end() {
    let mock = spawn(|p| {
        if p["t"] == "caps" {
            Reply::xml(CAPS)
        } else {
            Reply::xml(include_str!("data/torznab_movies.xml"))
        }
    })
    .await;
    let c = client(&mock.base, Flavor::Torznab);
    let feed = c.fetch_recent(None).await.unwrap();
    assert_eq!(feed.releases.len(), 3);
    assert!(
        feed.releases
            .iter()
            .all(|r| r.protocol == DownloadProtocol::Torrent && r.torrent.is_some())
    );
}

// ---- failures ------------------------------------------------------------------------

async fn fail_with(reply: impl Fn() -> Reply + Send + Sync + 'static) -> IndexerError {
    let mock = spawn(caps_or(move |_| reply())).await;
    client(&mock.base, Flavor::Newznab)
        .fetch_recent(None)
        .await
        .unwrap_err()
}

#[tokio::test]
async fn bad_api_key_is_reported_with_its_own_class() {
    let err =
        fail_with(|| Reply::xml(r#"<error code="100" description="Incorrect user credentials"/>"#))
            .await;
    assert!(matches!(err, IndexerError::ApiKey(_)), "{err:?}");
    let policy = err.backoff();
    assert!(policy.escalate && policy.minimum.is_none());
}

#[tokio::test]
async fn too_many_requests_carries_retry_after() {
    let err = fail_with(|| Reply {
        status: 429,
        content_type: "text/plain",
        body: "slow down".into(),
        headers: vec![("retry-after", "120".into())],
    })
    .await;
    assert!(
        matches!(err, IndexerError::TooManyRequests { retry_after: Some(d) } if d == Duration::from_secs(120)),
        "{err:?}"
    );
    assert_eq!(err.backoff().minimum, Some(Duration::from_secs(120)));

    let no_header = IndexerError::TooManyRequests { retry_after: None };
    assert_eq!(
        no_header.backoff().minimum,
        Some(Duration::from_secs(3600)),
        "default is one hour"
    );
    assert_eq!(
        IndexerError::RequestLimit { retry_after: None }
            .backoff()
            .minimum,
        Some(Duration::from_secs(3600))
    );
}

#[tokio::test]
async fn server_errors_and_html_pages() {
    let err = fail_with(|| Reply {
        status: 503,
        content_type: "text/plain",
        body: "down".into(),
        headers: vec![],
    })
    .await;
    assert!(matches!(err, IndexerError::Http(503)), "{err:?}");
    let err = fail_with(|| Reply {
        status: 200,
        content_type: "text/html; charset=utf-8",
        body: "<html>Cloudflare</html>".into(),
        headers: vec![],
    })
    .await;
    assert!(matches!(err, IndexerError::Html), "{err:?}");
}

#[tokio::test]
async fn an_xml_error_on_a_4xx_status_is_still_understood() {
    let err = fail_with(|| Reply {
        status: 401,
        content_type: "text/xml",
        body: r#"<error code="100" description="Incorrect user credentials"/>"#.into(),
        headers: vec![],
    })
    .await;
    assert!(matches!(err, IndexerError::ApiKey(_)), "{err:?}");
}

#[tokio::test]
async fn connection_failures_hold_the_back_off_level() {
    // Nothing listens on this port.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let err = client(&base, Flavor::Newznab)
        .fetch_recent(None)
        .await
        .unwrap_err();
    assert!(matches!(err, IndexerError::Network(_)), "{err:?}");
    assert!(!err.backoff().escalate);
}

#[tokio::test]
async fn caps_errors_fall_back_to_defaults_but_a_bad_key_does_not() {
    let mock = spawn(|p| {
        if p["t"] == "caps" {
            Reply::xml(r#"<error code="900" description="boom"/>"#)
        } else {
            Reply::xml(feed(0, 1))
        }
    })
    .await;
    let c = client(&mock.base, Flavor::Newznab);
    assert_eq!(
        c.fetch_recent(None).await.unwrap().releases.len(),
        1,
        "defaults were used"
    );

    let mock = spawn(|p| {
        if p["t"] == "caps" {
            Reply::xml(r#"<error code="100" description="Incorrect user credentials"/>"#)
        } else {
            Reply::xml(feed(0, 1))
        }
    })
    .await;
    let err = client(&mock.base, Flavor::Newznab)
        .fetch_recent(None)
        .await
        .unwrap_err();
    assert!(matches!(err, IndexerError::ApiKey(_)));
}

#[tokio::test]
async fn test_connection() {
    let mock = spawn(caps_or(|_| Reply::xml(feed(0, 2)))).await;
    client(&mock.base, Flavor::Newznab).test().await.unwrap();
    let bad = spawn(caps_or(|_| Reply {
        status: 500,
        content_type: "text/plain",
        body: String::new(),
        headers: vec![],
    }))
    .await;
    assert!(client(&bad.base, Flavor::Newznab).test().await.is_err());
}

#[test]
fn invalid_settings_are_refused_up_front() {
    let settings = IndexerSettings {
        base_url: "not a url".into(),
        ..Default::default()
    };
    assert!(IndexerClient::new(Flavor::Newznab, settings, identity()).is_err());
}

#[tokio::test]
async fn the_rate_limit_spaces_requests_out() {
    let mock = spawn(caps_or(|_| Reply::xml(feed(0, 1)))).await;
    let settings = IndexerSettings {
        base_url: mock.base.clone(),
        categories: vec![2000],
        ..Default::default()
    };
    let c = IndexerClient::new(Flavor::Newznab, settings, identity())
        .unwrap()
        .with_rate_limit(Duration::from_millis(300));
    let start = std::time::Instant::now();
    c.fetch_recent(None).await.unwrap(); // caps + one page
    assert!(
        start.elapsed() >= Duration::from_millis(280),
        "{:?}",
        start.elapsed()
    );
}
