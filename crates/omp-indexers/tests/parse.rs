use omp_core::release::{DownloadProtocol, IndexerFlags};
use omp_indexers::{Capabilities, Flavor, IndexerError, parse_feed};

mod common;
use common::{source, source_with_url};

const NEWZNAB: &str = include_str!("data/newznab_movies.xml");
const TORZNAB: &str = include_str!("data/torznab_movies.xml");
const CAPS: &str = include_str!("data/newznab_caps.xml");

#[test]
fn newznab_items_map_to_releases() {
    let releases = parse_feed(NEWZNAB, &source(Flavor::Newznab)).unwrap();
    assert_eq!(releases.len(), 4);

    let first = &releases[0];
    assert_eq!(first.title, "The.Matrix.1999.1080p.BluRay.x264-GRP");
    assert_eq!(first.protocol, DownloadProtocol::Usenet);
    assert_eq!(
        first.size, 9_876_543_210,
        "the size attribute wins over the enclosure length"
    );
    assert_eq!(first.tmdb_id, 603);
    assert_eq!(first.imdb_id.as_deref(), Some("tt0133093"));
    assert_eq!(
        first.download_url,
        "https://idx.example/getnzb/aaa.nzb?i=1&r=KEY"
    );
    assert_eq!(first.guid, "https://idx.example/details/aaa");
    assert_eq!(first.indexer, "Example");
    assert_eq!(first.indexer_id, 7);
    assert_eq!(first.indexer_priority, 25);
    assert!(
        first.indexer_flags.contains(IndexerFlags::G_SCENE),
        "prematch=1"
    );
    assert_eq!(
        first.publish_date.to_rfc3339(),
        "2026-10-02T22:00:00+00:00",
        "usenetdate wins over pubDate"
    );
    assert!(first.torrent.is_none());
}

#[test]
fn newznab_fallbacks_and_oddities() {
    let releases = parse_feed(NEWZNAB, &source(Flavor::Newznab)).unwrap();
    let second = &releases[1];
    assert_eq!(
        second.size, 2_222_222,
        "no size attribute: enclosure length"
    );
    assert_eq!(
        second.download_url, "https://idx.example/getnzb/bbb.nzb",
        "relative link resolved"
    );
    assert!(second.indexer_flags.contains(IndexerFlags::NUKED));
    assert_eq!(
        second.publish_date.to_rfc3339(),
        "2026-10-03T09:00:00+00:00"
    );

    let third = &releases[2];
    assert_eq!(
        third.title, "Fish & Chips\u{a0}(2020) 1080p WEB-DL",
        "entities decoded"
    );

    let fourth = &releases[3];
    assert!(
        fourth.download_url.is_empty(),
        "no enclosure, so no link (the client drops it)"
    );
}

#[test]
fn torznab_items_carry_torrent_details_and_flags() {
    let releases = parse_feed(TORZNAB, &source(Flavor::Torznab)).unwrap();
    assert_eq!(releases.len(), 3);

    let first = &releases[0];
    assert_eq!(first.protocol, DownloadProtocol::Torrent);
    let t = first.torrent.as_ref().unwrap();
    assert_eq!(t.seeders, Some(120));
    assert_eq!(t.peers, Some(150));
    assert_eq!(
        t.info_hash.as_deref(),
        Some("ABCDEF0123456789ABCDEF0123456789ABCDEF01")
    );
    assert!(
        t.magnet_url
            .as_deref()
            .unwrap()
            .starts_with("magnet:?xt=urn:btih:")
    );
    assert_eq!(first.size, 55_000_000_000);
    assert_eq!(first.tmdb_id, 603);
    assert_eq!(first.imdb_id.as_deref(), Some("tt0133093"));
    for flag in [
        IndexerFlags::G_FREELEECH,
        IndexerFlags::G_DOUBLE_UPLOAD,
        IndexerFlags::G_INTERNAL,
        IndexerFlags::G_SCENE,
    ] {
        assert!(first.indexer_flags.contains(flag), "{flag:?}");
    }

    let second = &releases[1];
    assert_eq!(
        second.torrent.as_ref().unwrap().peers,
        Some(10),
        "peers = seeders + leechers"
    );
    assert!(
        second.indexer_flags.contains(IndexerFlags::G_FREELEECH25),
        "0.75 download factor"
    );
    assert!(!second.indexer_flags.contains(IndexerFlags::G_FREELEECH));

    assert!(
        releases[2]
            .indexer_flags
            .contains(IndexerFlags::G_HALFLEECH)
    );
    assert_eq!(releases[2].torrent.as_ref().unwrap().seeders, None);
}

#[test]
fn a_feed_of_the_wrong_kind_yields_nothing() {
    assert!(
        parse_feed(TORZNAB, &source(Flavor::Newznab))
            .unwrap()
            .is_empty(),
        "torrents from a Newznab indexer"
    );
    assert!(
        parse_feed(NEWZNAB, &source(Flavor::Torznab))
            .unwrap()
            .is_empty(),
        "nzbs from a Torznab indexer"
    );
}

#[test]
fn error_elements_are_classified() {
    let err = |code: &str, desc: &str| {
        format!(r#"<?xml version="1.0"?><error code="{code}" description="{desc}"/>"#)
    };
    let parse = |xml: String, url: &str| {
        parse_feed(&xml, &source_with_url(Flavor::Newznab, url)).unwrap_err()
    };
    assert!(matches!(
        parse(
            err("100", "Incorrect user credentials"),
            "https://x/api?apikey=k"
        ),
        IndexerError::ApiKey(_)
    ));
    assert!(matches!(
        parse(err("101", "Account suspended"), "https://x/api?apikey=k"),
        IndexerError::ApiKey(_)
    ));
    assert!(
        matches!(
            parse(err("200", "Missing parameter"), "https://x/api?t=movie"),
            IndexerError::ApiKey(_)
        ),
        "no key was sent"
    );
    assert!(matches!(
        parse(err("200", "Missing parameter"), "https://x/api?apikey=k"),
        IndexerError::Api { .. }
    ));
    assert!(matches!(
        parse(
            err("500", "Request limit reached"),
            "https://x/api?apikey=k"
        ),
        IndexerError::RequestLimit { .. }
    ));
    assert!(matches!(
        parse(err("300", "No such function"), "https://x/api?apikey=k"),
        IndexerError::Api {
            code: Some(300),
            ..
        }
    ));
}

#[test]
fn an_item_without_a_date_rejects_the_whole_feed() {
    let xml = r#"<rss><channel><item><title>x</title><enclosure url="https://x/a.nzb" length="1" type="application/x-nzb"/></item></channel></rss>"#;
    assert!(matches!(
        parse_feed(xml, &source(Flavor::Newznab)),
        Err(IndexerError::UnsupportedFeed(_))
    ));
}

#[test]
fn an_unreadable_date_skips_just_that_item() {
    let xml = r#"<rss><channel>
        <item><title>bad</title><pubDate>whenever</pubDate><enclosure url="https://x/a.nzb" length="1" type="application/x-nzb"/></item>
        <item><title>good</title><pubDate>Sat, 03 Oct 2026 09:00:00 +0000</pubDate><enclosure url="https://x/b.nzb" length="1" type="application/x-nzb"/></item>
        </channel></rss>"#;
    let releases = parse_feed(xml, &source(Flavor::Newznab)).unwrap();
    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0].title, "good");
}

#[test]
fn garbage_is_a_parse_error() {
    assert!(matches!(
        parse_feed("not xml at all", &source(Flavor::Newznab)),
        Err(IndexerError::Parse(_))
    ));
    assert!(
        parse_feed("<rss><nochannel/></rss>", &source(Flavor::Newznab))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn capabilities_parse() {
    let caps = Capabilities::parse(CAPS).unwrap();
    assert_eq!((caps.default_page_size, caps.max_page_size), (50, 100));
    assert!(caps.supports_text_search());
    assert!(caps.supports_tmdb_search() && caps.supports_imdb_search());
    assert!(caps.supports_aggregate_id_search);
    assert_eq!(caps.text_search_engine, "sphinx");
    assert_eq!(caps.movie_text_search_engine, "raw");
    assert_eq!(caps.categories.len(), 2);
    assert_eq!(caps.categories[0].subcategories.len(), 2);
    assert_eq!(caps.categories[0].subcategories[1].description, "HD movies");
}

#[test]
fn capabilities_unavailable_features_and_errors() {
    let none = r#"<caps><searching><search available="no"/><movie-search available="no"/></searching></caps>"#;
    let caps = Capabilities::parse(none).unwrap();
    assert!(caps.search_params.is_none() && caps.movie_params.is_none());
    assert!(!caps.supports_text_search());

    assert!(matches!(
        Capabilities::parse(r#"<error code="100" description="Incorrect user credentials"/>"#),
        Err(IndexerError::ApiKey(_))
    ));
    assert!(matches!(
        Capabilities::parse("<html/>"),
        Err(IndexerError::Parse(_))
    ));
    assert_eq!(Capabilities::default().movie_params.unwrap().len(), 4);
}
