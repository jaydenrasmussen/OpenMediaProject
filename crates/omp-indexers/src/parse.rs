//! Turning a Newznab/Torznab RSS response into releases. Ported from Radarr
//! `RssParser`, `NewznabRssParser`, `TorrentRssParser` and `TorznabRssParser`
//! (GPL-3.0).

use std::sync::LazyLock;

use chrono::{DateTime, NaiveDateTime, Utc};
use omp_core::release::{DownloadProtocol, IndexerFlags, ReleaseInfo, TorrentRelease};
use omp_parser::re::Re;
use roxmltree::{Document, Node};

use crate::{error::IndexerError, settings::Flavor};

const NEWZNAB_NS: &str = "http://www.newznab.com/DTD/2010/feeds/attributes/";
const TORZNAB_NS: &str = "http://torznab.com/schemas/2015/feed";
const NZB_MIME: &str = "application/x-nzb";
const TORRENT_MIMES: [&str; 2] = [
    "application/x-bittorrent",
    "application/x-bittorrent;x-scheme-handler/magnet",
];

static ENTITY: LazyLock<Re> = LazyLock::new(|| Re::ci(r"&[a-z]+;"));

/// Makes sloppy indexer XML parseable: named HTML entities (`&nbsp;`) become
/// numeric references, and characters XML forbids are dropped.
pub fn clean_xml(content: &str) -> String {
    let with_entities = ENTITY.replace_with(content, |c| {
        let entity = c.whole_str();
        match entity.to_lowercase().as_str() {
            "&amp;" | "&lt;" | "&gt;" | "&quot;" | "&apos;" => entity.to_owned(),
            _ => {
                let decoded = html_escape::decode_html_entities(entity);
                match decoded.chars().next() {
                    Some(ch) if decoded.as_ref() != entity => format!("&#{};", ch as u32),
                    _ => entity.to_owned(),
                }
            }
        }
    });
    with_entities
        .chars()
        .filter(|c| {
            matches!(*c, '\u{09}' | '\u{0A}' | '\u{0D}')
                || ('\u{20}'..='\u{D7FF}').contains(c)
                || ('\u{E000}'..='\u{FFFD}').contains(c)
                || *c > '\u{FFFF}'
        })
        .collect()
}

/// Looks for an `<error code=".." description=".."/>` and maps it.
pub(crate) fn check_error(doc: &Document<'_>, request_url: &str) -> Result<(), IndexerError> {
    let Some(error) = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "error")
    else {
        return Ok(());
    };
    let code = error.attribute("code").and_then(|c| c.parse::<i32>().ok());
    let message = error
        .attribute("description")
        .unwrap_or_default()
        .to_owned();

    if code.is_some_and(|c| (100..=199).contains(&c)) {
        return Err(IndexerError::ApiKey(message));
    }
    if !request_url.contains("apikey=")
        && (message == "Missing parameter" || message.contains("apikey"))
    {
        return Err(IndexerError::ApiKey("Indexer requires an API key".into()));
    }
    if message == "Request limit reached" {
        return Err(IndexerError::RequestLimit { retry_after: None });
    }
    Err(IndexerError::Api { code, message })
}

/// Which indexer the releases came from.
pub struct Source<'a> {
    pub id: i32,
    pub name: &'a str,
    pub priority: i32,
    pub flavor: Flavor,
    /// The URL that was requested, for resolving relative links.
    pub request_url: &'a str,
    /// The indexer's seeding goals, passed on with every torrent it lists.
    pub seed_ratio: Option<f64>,
    pub seed_time_minutes: Option<u32>,
}

/// `Size: 1.3 GB` / `1.3 GiB` / plain digits to bytes.
pub fn parse_size(text: &str, default_binary: bool) -> u64 {
    static SIZE: LazyLock<Re> = LazyLock::new(|| {
        Re::ci(r"(?<value>(?<!\.\d*)(?:\d+,)*\d+(?:\.\d{1,3})?)\W?(?<unit>[KMG]i?B)(?![\w/])")
    });
    let text = text.trim();
    if text.is_empty() {
        return 0;
    }
    if text.chars().all(|c| c.is_ascii_digit()) {
        return text.parse().unwrap_or(0);
    }
    let Some(m) = SIZE.captures(text) else {
        return 0;
    };
    let value: f64 = m
        .name("value")
        .unwrap_or("0")
        .replace(',', "")
        .parse()
        .unwrap_or(0.0);
    let unit = m.name("unit").unwrap_or_default().to_lowercase();
    let (power, binary) = match unit.as_str() {
        "kb" => (1, default_binary),
        "mb" => (2, default_binary),
        "gb" => (3, default_binary),
        "kib" => (1, true),
        "mib" => (2, true),
        "gib" => (3, true),
        _ => return value as u64,
    };
    let base: f64 = if binary { 1024.0 } else { 1000.0 };
    (value * base.powi(power)).round() as u64
}

fn parse_date(text: &str) -> Option<DateTime<Utc>> {
    let text = text.trim();
    if let Ok(d) = DateTime::parse_from_rfc2822(text) {
        return Some(d.with_timezone(&Utc));
    }
    if let Ok(d) = DateTime::parse_from_rfc3339(text) {
        return Some(d.with_timezone(&Utc));
    }

    // Feeds often get the weekday wrong ("Sat" for a Friday), which a strict
    // parser rejects. The date itself is still good, so retry without it.
    static WEEKDAY: LazyLock<Re> = LazyLock::new(|| Re::ci(r"^[a-z]{3,9},?\s+"));
    let without_weekday = WEEKDAY.replace_all(text, "");
    if let Ok(d) = DateTime::parse_from_str(&without_weekday, "%d %b %Y %H:%M:%S %z") {
        return Some(d.with_timezone(&Utc));
    }
    // An unknown zone name ("... XYZ"): drop it and read the time as UTC,
    // as Radarr does.
    static TZ_NAME: LazyLock<Re> = LazyLock::new(|| Re::new(r"\s+[A-Z]{2,5}$"));
    let stripped = TZ_NAME.replace_all(&without_weekday, "");
    for fmt in ["%d %b %Y %H:%M:%S", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(n) = NaiveDateTime::parse_from_str(stripped.trim(), fmt) {
            return Some(n.and_utc());
        }
    }
    None
}

struct Item<'a, 'i> {
    node: Node<'a, 'i>,
    attr_ns: &'static str,
}

impl<'a, 'i> Item<'a, 'i> {
    fn text(&self, name: &str) -> Option<&'a str> {
        self.node
            .children()
            .find(|n| {
                n.is_element() && n.tag_name().name() == name && n.tag_name().namespace().is_none()
            })
            .and_then(|n| n.text())
            .map(str::trim)
    }

    fn attrs(&self, key: &str) -> Vec<&'a str> {
        self.node
            .children()
            .filter(|n| {
                n.is_element()
                    && n.tag_name().name() == "attr"
                    && n.tag_name().namespace() == Some(self.attr_ns)
                    && n.attribute("name")
                        .is_some_and(|k| k.eq_ignore_ascii_case(key))
            })
            .filter_map(|n| n.attribute("value"))
            .collect()
    }

    fn attr(&self, key: &str) -> Option<&'a str> {
        self.attrs(key).into_iter().next()
    }

    fn enclosures(&self) -> Vec<Enclosure<'a>> {
        self.node
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "enclosure")
            .map(|n| Enclosure {
                url: n.attribute("url"),
                mime: n.attribute("type"),
                length: n
                    .attribute("length")
                    .and_then(|l| l.trim().parse().ok())
                    .unwrap_or(0),
            })
            .collect()
    }
}

struct Enclosure<'a> {
    url: Option<&'a str>,
    mime: Option<&'a str>,
    length: u64,
}

fn preferred_enclosure<'a>(
    enclosures: &'a [Enclosure<'a>],
    preferred: &[&str],
) -> Option<&'a Enclosure<'a>> {
    preferred
        .iter()
        .find_map(|p| enclosures.iter().find(|e| e.mime == Some(*p)))
}

fn resolve(request_url: &str, value: &str) -> Option<String> {
    if value.trim().is_empty() {
        return None;
    }
    if value.starts_with("magnet:") {
        return Some(value.to_owned());
    }
    let base = url::Url::parse(request_url).ok();
    match (url::Url::parse(value), base) {
        (Ok(abs), _) => Some(abs.to_string()),
        (Err(_), Some(base)) => base.join(value).ok().map(|u| u.to_string()),
        _ => None,
    }
}

fn imdb_id(value: Option<&str>) -> Option<String> {
    let n: u64 = value?.trim().trim_start_matches("tt").parse().ok()?;
    (n > 0).then(|| format!("tt{n:07}"))
}

fn newznab_flags(item: &Item<'_, '_>) -> IndexerFlags {
    let mut flags = IndexerFlags::NONE;
    if item.attr("prematch") == Some("1") || item.attr("haspretime") == Some("1") {
        flags = flags | IndexerFlags::G_SCENE;
    }
    if item.attr("nuked") == Some("1") {
        flags = flags | IndexerFlags::NUKED;
    }
    flags
}

fn torznab_flags(item: &Item<'_, '_>) -> IndexerFlags {
    let factor = |key: &str| {
        item.attr(key)
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(1.0)
    };
    let (download, upload) = (factor("downloadvolumefactor"), factor("uploadvolumefactor"));
    let mut flags = IndexerFlags::NONE;
    // Named for how much is waived: "Freeleech75" costs 25% of the download.
    if download == 0.5 {
        flags = flags | IndexerFlags::G_HALFLEECH;
    }
    if download == 0.75 {
        flags = flags | IndexerFlags::G_FREELEECH25;
    }
    if download == 0.25 {
        flags = flags | IndexerFlags::G_FREELEECH75;
    }
    if download == 0.0 {
        flags = flags | IndexerFlags::G_FREELEECH;
    }
    if upload == 2.0 {
        flags = flags | IndexerFlags::G_DOUBLE_UPLOAD;
    }
    let tags = item.attrs("tag");
    if tags.iter().any(|t| t.eq_ignore_ascii_case("internal")) {
        flags = flags | IndexerFlags::G_INTERNAL;
    }
    if tags.iter().any(|t| t.eq_ignore_ascii_case("scene")) {
        flags = flags | IndexerFlags::G_SCENE;
    }
    flags
}

/// Parses a search or RSS response. Items that cannot be read are skipped;
/// a feed whose items have no usable date is rejected as a whole.
pub fn parse_feed(xml: &str, source: &Source<'_>) -> Result<Vec<ReleaseInfo>, IndexerError> {
    let cleaned = clean_xml(xml);
    let doc = Document::parse(&cleaned).map_err(|e| IndexerError::Parse(e.to_string()))?;
    check_error(&doc, source.request_url)?;

    let (attr_ns, preferred, protocol): (&'static str, &[&str], DownloadProtocol) =
        match source.flavor {
            Flavor::Newznab => (NEWZNAB_NS, &[NZB_MIME], DownloadProtocol::Usenet),
            Flavor::Torznab => (TORZNAB_NS, &TORRENT_MIMES, DownloadProtocol::Torrent),
        };

    let Some(channel) = doc
        .root_element()
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "channel")
    else {
        return Ok(vec![]);
    };
    let items: Vec<Item<'_, '_>> = channel
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "item")
        .map(|node| Item { node, attr_ns })
        .collect();

    // A feed of the wrong kind (torrent enclosures from a Newznab indexer) is
    // reported and ignored rather than half-parsed.
    let mimes: Vec<&str> = items
        .iter()
        .flat_map(|i| i.enclosures())
        .filter_map(|e| e.mime)
        .collect();
    if !mimes.is_empty() && !mimes.iter().any(|m| preferred.contains(m)) {
        tracing::warn!(
            indexer = source.name,
            found = mimes[0],
            "feed has no {} enclosures; is this the right indexer type?",
            preferred[0]
        );
        return Ok(vec![]);
    }

    let mut releases = Vec::with_capacity(items.len());
    for item in &items {
        match parse_item(item, source, preferred, protocol) {
            Ok(Some(release)) => releases.push(release),
            Ok(None) => {}
            Err(e @ IndexerError::UnsupportedFeed(_)) => return Err(e),
            Err(e) => tracing::warn!(indexer = source.name, "skipping a feed item: {e}"),
        }
    }
    Ok(releases)
}

fn parse_item(
    item: &Item<'_, '_>,
    source: &Source<'_>,
    preferred: &[&str],
    protocol: DownloadProtocol,
) -> Result<Option<ReleaseInfo>, IndexerError> {
    let enclosures = item.enclosures();
    let enclosure = preferred_enclosure(&enclosures, preferred);

    // Newznab prefers its own usenet date; everything else uses pubDate.
    let date_text = if source.flavor == Flavor::Newznab {
        item.attr("usenetdate")
            .filter(|d| !d.trim().is_empty())
            .or_else(|| item.text("pubDate"))
    } else {
        item.text("pubDate")
    };
    let publish_date = match date_text {
        None | Some("") => {
            return Err(IndexerError::UnsupportedFeed(
                "Rss feed must have a pubDate element with a valid publish date.".into(),
            ));
        }
        Some(text) => parse_date(text)
            .ok_or_else(|| IndexerError::Parse(format!("unreadable date {text:?}")))?,
    };

    // Download link: the preferred enclosure, else any enclosure.
    let download_url = enclosure
        .and_then(|e| e.url)
        .or_else(|| enclosures.first().and_then(|e| e.url))
        .and_then(|u| resolve(source.request_url, u));

    let size = item
        .attr("size")
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or_else(|| enclosure.or(enclosures.first()).map_or(0, |e| e.length));

    let title = item
        .text("title")
        .filter(|t| !t.is_empty())
        .unwrap_or("Unknown")
        .to_owned();
    let guid = item
        .text("guid")
        .filter(|g| !g.is_empty())
        .map_or_else(|| format!("{}:{title}", source.id), str::to_owned);

    let tmdb_id = item
        .attr("tmdbid")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    let imdb = imdb_id(item.attr("imdb"));

    let (indexer_flags, torrent) = match source.flavor {
        Flavor::Newznab => (newznab_flags(item), None),
        Flavor::Torznab => {
            let seeders = item
                .attr("seeders")
                .and_then(|v| v.trim().parse::<i32>().ok());
            let peers = item
                .attr("peers")
                .and_then(|v| v.trim().parse::<i32>().ok())
                .or_else(|| {
                    let leechers = item.attr("leechers")?.trim().parse::<i32>().ok()?;
                    Some(seeders? + leechers)
                });
            let magnet = item
                .attr("magneturl")
                .filter(|m| !m.is_empty())
                .map(str::to_owned);
            let info_hash = item
                .attr("infohash")
                .filter(|h| !h.is_empty())
                .map(str::to_owned);
            (
                torznab_flags(item),
                Some(TorrentRelease {
                    magnet_url: magnet,
                    info_hash,
                    seeders,
                    peers,
                    seed_ratio: source.seed_ratio,
                    seed_time_minutes: source.seed_time_minutes,
                }),
            )
        }
    };

    Ok(Some(ReleaseInfo {
        guid,
        title,
        size,
        download_url: download_url.unwrap_or_default(),
        indexer_id: source.id,
        indexer: source.name.to_owned(),
        indexer_priority: source.priority,
        protocol,
        publish_date,
        container: None,
        indexer_flags,
        imdb_id: imdb,
        tmdb_id,
        torrent,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_parsing() {
        assert_eq!(parse_size("1234", true), 1234);
        assert_eq!(parse_size("Size: 1.5 GB", true), 1_610_612_736);
        assert_eq!(parse_size("1.5 GB", false), 1_500_000_000);
        assert_eq!(
            parse_size("1.5 GiB", false),
            1_610_612_736,
            "GiB is always binary"
        );
        assert_eq!(parse_size("700 MB", true), 734_003_200);
        assert_eq!(parse_size("1,024 KB", false), 1_024_000);
        assert_eq!(parse_size("no size here", true), 0);
        assert_eq!(parse_size("", true), 0);
    }

    #[test]
    fn xml_cleaning() {
        let cleaned = clean_xml("<a>Fish &amp; Chips &nbsp;&copy; \u{0001}ok</a>");
        assert!(cleaned.contains("&amp;"), "XML entities are kept");
        assert!(
            cleaned.contains("&#160;") && cleaned.contains("&#169;"),
            "{cleaned}"
        );
        assert!(!cleaned.contains('\u{1}'));
        assert!(roxmltree::Document::parse(&cleaned).is_ok());
        assert_eq!(
            clean_xml("&unknownentity;"),
            "&unknownentity;",
            "undecodable entities are left alone"
        );
    }

    #[test]
    fn dates() {
        let d = parse_date("Fri, 16 Oct 2026 10:30:00 +0200").unwrap();
        assert_eq!(d.to_rfc3339(), "2026-10-16T08:30:00+00:00");
        let wrong_weekday = parse_date("Sat, 16 Oct 2026 10:30:00 +0200").unwrap();
        assert_eq!(wrong_weekday, d, "a wrong weekday is ignored");
        assert!(parse_date("2026-10-16T08:30:00Z").is_some());
        let tz_name = parse_date("Sat, 16 Oct 2026 10:30:00 PST").unwrap();
        assert_eq!(
            tz_name.to_rfc3339(),
            "2026-10-16T10:30:00+00:00",
            "unknown zone dropped, read as UTC"
        );
        assert!(parse_date("yesterday").is_none());
    }

    #[test]
    fn imdb_ids() {
        assert_eq!(imdb_id(Some("133093")).as_deref(), Some("tt0133093"));
        assert_eq!(imdb_id(Some("tt0133093")).as_deref(), Some("tt0133093"));
        assert_eq!(imdb_id(Some("0")), None);
        assert_eq!(imdb_id(None), None);
    }

    #[test]
    fn relative_links_resolve_against_the_request() {
        assert_eq!(
            resolve("https://idx.example/api?t=movie", "/getnzb/abc.nzb").as_deref(),
            Some("https://idx.example/getnzb/abc.nzb")
        );
        assert_eq!(
            resolve("https://idx.example/api", "https://cdn.example/x.nzb").as_deref(),
            Some("https://cdn.example/x.nzb")
        );
        assert_eq!(
            resolve("https://x/api", "magnet:?xt=urn:btih:abc").as_deref(),
            Some("magnet:?xt=urn:btih:abc")
        );
        assert_eq!(resolve("https://x/api", "  "), None);
    }
}
