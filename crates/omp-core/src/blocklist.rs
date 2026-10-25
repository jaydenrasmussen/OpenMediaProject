//! Blocklist matching. Ported from Radarr `BlocklistService.Blocklisted` and
//! `ReleaseComparer` (GPL-3.0).

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    language::Language,
    quality::QualityModel,
    release::{DownloadProtocol, IndexerFlags, ReleaseInfo},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlocklistEntry {
    pub id: i64,
    pub movie_id: i64,
    pub source_title: String,
    pub quality: QualityModel,
    pub languages: Vec<Language>,
    pub published_date: Option<DateTime<Utc>>,
    pub size: Option<u64>,
    pub protocol: DownloadProtocol,
    pub indexer: Option<String>,
    pub indexer_flags: IndexerFlags,
    pub torrent_info_hash: Option<String>,
    pub message: Option<String>,
    pub date: DateTime<Utc>,
}

fn blank(s: &Option<String>) -> bool {
    s.as_deref().is_none_or(|s| s.trim().is_empty())
}

fn same_indexer(entry: &BlocklistEntry, indexer: &str) -> bool {
    blank(&entry.indexer)
        || entry
            .indexer
            .as_deref()
            .is_some_and(|i| i.to_lowercase() == indexer.to_lowercase())
}

fn same_published_date(entry: &BlocklistEntry, published: DateTime<Utc>) -> bool {
    match entry.published_date {
        None => true,
        Some(d) => d - Duration::minutes(2) <= published && d + Duration::minutes(2) >= published,
    }
}

fn same_size(entry: &BlocklistEntry, size: u64) -> bool {
    let entry_size = entry.size.unwrap_or(0);
    if entry_size == 0 {
        return true;
    }
    entry_size.abs_diff(size) <= 2 * 1024 * 1024
}

/// Upstream quirk, kept for parity: the second clause asks for a *different*
/// indexer (`!HasSameIndexer`). In practice the first clause (identical publish
/// date) is what matches.
fn same_nzb(entry: &BlocklistEntry, release: &ReleaseInfo) -> bool {
    if entry.published_date == Some(release.publish_date) {
        return true;
    }
    !same_indexer(entry, &release.indexer)
        && same_published_date(entry, release.publish_date)
        && same_size(entry, release.size)
}

fn same_torrent(entry: &BlocklistEntry, release: &ReleaseInfo) -> bool {
    let hash = release
        .torrent
        .as_ref()
        .and_then(|t| t.info_hash.as_deref());
    match hash.filter(|h| !h.trim().is_empty()) {
        Some(hash) => entry
            .torrent_info_hash
            .as_deref()
            .is_some_and(|h| h.to_lowercase() == hash.to_lowercase()),
        None => same_indexer(entry, &release.indexer),
    }
}

/// Is `release` blocklisted, given the blocklist entries for its movie?
pub fn is_blocklisted(entries: &[BlocklistEntry], release: &ReleaseInfo) -> bool {
    let title = release.title.to_lowercase();
    match release.protocol {
        DownloadProtocol::Torrent => {
            let Some(torrent) = &release.torrent else {
                return false;
            };
            if let Some(hash) = torrent
                .info_hash
                .as_deref()
                .filter(|h| !h.trim().is_empty())
            {
                return entries.iter().any(|e| {
                    e.torrent_info_hash
                        .as_deref()
                        .is_some_and(|h| h.to_lowercase() == hash.to_lowercase())
                        && same_torrent(e, release)
                });
            }
            entries.iter().any(|e| {
                e.source_title.to_lowercase() == title
                    && e.protocol == DownloadProtocol::Torrent
                    && same_torrent(e, release)
            })
        }
        _ => entries.iter().any(|e| {
            e.source_title.to_lowercase() == title
                && e.protocol == DownloadProtocol::Usenet
                && same_nzb(e, release)
        }),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::release::TorrentRelease;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 1, 12, 0, 0).unwrap()
    }

    fn release(protocol: DownloadProtocol) -> ReleaseInfo {
        ReleaseInfo {
            guid: String::new(),
            title: "Movie.2020.1080p.WEB-DL-GRP".into(),
            size: 4_000_000_000,
            download_url: String::new(),
            indexer_id: 1,
            indexer: "IndexerA".into(),
            indexer_priority: 25,
            protocol,
            publish_date: now(),
            container: None,
            indexer_flags: IndexerFlags::NONE,
            imdb_id: None,
            tmdb_id: 0,
            torrent: None,
        }
    }

    fn entry(protocol: DownloadProtocol) -> BlocklistEntry {
        BlocklistEntry {
            id: 1,
            movie_id: 1,
            source_title: "movie.2020.1080p.web-dl-grp".into(),
            quality: Default::default(),
            languages: vec![],
            published_date: Some(now()),
            size: Some(4_000_000_000),
            protocol,
            indexer: Some("IndexerA".into()),
            indexer_flags: IndexerFlags::NONE,
            torrent_info_hash: None,
            message: None,
            date: now(),
        }
    }

    #[test]
    fn usenet_matches_by_title_and_identical_publish_date() {
        let r = release(DownloadProtocol::Usenet);
        assert!(is_blocklisted(&[entry(DownloadProtocol::Usenet)], &r));

        let mut other_title = entry(DownloadProtocol::Usenet);
        other_title.source_title = "something else".into();
        assert!(!is_blocklisted(&[other_title], &r));

        let mut torrent_entry = entry(DownloadProtocol::Torrent);
        torrent_entry.torrent_info_hash = Some("ABC".into());
        assert!(!is_blocklisted(&[torrent_entry], &r), "wrong protocol");
    }

    #[test]
    fn usenet_second_clause_needs_a_different_indexer() {
        let mut r = release(DownloadProtocol::Usenet);
        r.publish_date = now() + Duration::minutes(1); // not identical, within 2 minutes
        let mut e = entry(DownloadProtocol::Usenet);
        assert!(
            !is_blocklisted(std::slice::from_ref(&e), &r),
            "same indexer: upstream does not match"
        );
        e.indexer = Some("IndexerB".into());
        assert!(
            is_blocklisted(std::slice::from_ref(&e), &r),
            "different indexer, close date and size"
        );
        r.size += 10 * 1024 * 1024;
        assert!(
            !is_blocklisted(std::slice::from_ref(&e), &r),
            "size differs by more than 2 MB"
        );
    }

    #[test]
    fn torrents_match_by_hash_ignoring_case() {
        let mut r = release(DownloadProtocol::Torrent);
        r.torrent = Some(TorrentRelease {
            info_hash: Some("abcdef".into()),
            ..Default::default()
        });
        let mut e = entry(DownloadProtocol::Torrent);
        e.torrent_info_hash = Some("ABCDEF".into());
        assert!(is_blocklisted(std::slice::from_ref(&e), &r));
        e.torrent_info_hash = Some("OTHER".into());
        assert!(!is_blocklisted(std::slice::from_ref(&e), &r));
    }

    #[test]
    fn torrents_without_a_hash_match_by_title_and_indexer() {
        let mut r = release(DownloadProtocol::Torrent);
        r.torrent = Some(TorrentRelease::default());
        let e = entry(DownloadProtocol::Torrent);
        assert!(is_blocklisted(std::slice::from_ref(&e), &r));
        let mut other = e.clone();
        other.indexer = Some("IndexerB".into());
        assert!(!is_blocklisted(&[other], &r));
        let mut blank_indexer = e;
        blank_indexer.indexer = None;
        assert!(
            is_blocklisted(&[blank_indexer], &r),
            "an entry with no indexer matches any"
        );
    }

    #[test]
    fn a_torrent_protocol_release_without_torrent_info_is_never_blocked() {
        let r = release(DownloadProtocol::Torrent);
        assert!(!is_blocklisted(&[entry(DownloadProtocol::Torrent)], &r));
    }
}
