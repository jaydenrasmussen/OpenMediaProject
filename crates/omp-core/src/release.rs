//! Release-level types shared by indexers, decisions and downloads.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Which kind of downloader a release is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum DownloadProtocol {
    #[default]
    Unknown,
    Usenet,
    Torrent,
}

/// Flags an indexer attaches to a release (Radarr `IndexerFlags`, GPL-3.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IndexerFlags(pub u32);

impl IndexerFlags {
    pub const NONE: IndexerFlags = IndexerFlags(0);
    /// Torrent download amount does not count.
    pub const G_FREELEECH: IndexerFlags = IndexerFlags(1);
    /// Torrent download amount only counts 50%.
    pub const G_HALFLEECH: IndexerFlags = IndexerFlags(2);
    /// Torrent upload amount is doubled.
    pub const G_DOUBLE_UPLOAD: IndexerFlags = IndexerFlags(4);
    /// Very high quality encode, marked manually by PTP staff.
    pub const PTP_GOLDEN: IndexerFlags = IndexerFlags(8);
    /// Checked by PTP staff or checkers for release description requirements.
    pub const PTP_APPROVED: IndexerFlags = IndexerFlags(16);
    /// Uploader is an internal release group.
    pub const G_INTERNAL: IndexerFlags = IndexerFlags(32);
    pub const AHD_INTERNAL: IndexerFlags = IndexerFlags(64);
    /// The release comes from a scene group.
    pub const G_SCENE: IndexerFlags = IndexerFlags(128);
    /// Torrent download amount only counts 75%.
    pub const G_FREELEECH75: IndexerFlags = IndexerFlags(256);
    /// Torrent download amount only counts 25%.
    pub const G_FREELEECH25: IndexerFlags = IndexerFlags(512);
    pub const AHD_USER_RELEASE: IndexerFlags = IndexerFlags(1024);
    pub const NUKED: IndexerFlags = IndexerFlags(2048);

    /// True when every bit of `flag` is set (`Enum.HasFlag`).
    pub fn contains(self, flag: IndexerFlags) -> bool {
        self.0 & flag.0 == flag.0
    }

    pub fn bits(self) -> u32 {
        self.0
    }
}

impl std::ops::BitOr for IndexerFlags {
    type Output = IndexerFlags;
    fn bitor(self, rhs: Self) -> Self {
        IndexerFlags(self.0 | rhs.0)
    }
}

/// Torrent-only details of a release.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TorrentRelease {
    pub magnet_url: Option<String>,
    pub info_hash: Option<String>,
    pub seeders: Option<i32>,
    pub peers: Option<i32>,
    /// The indexer's own seeding goals (private trackers set them); they
    /// replace the download client's for this torrent.
    pub seed_ratio: Option<f64>,
    pub seed_time_minutes: Option<u32>,
}

/// A release offered by an indexer (Radarr `ReleaseInfo`/`TorrentInfo`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub guid: String,
    pub title: String,
    /// Bytes; 0 when the indexer does not say.
    pub size: u64,
    pub download_url: String,
    pub indexer_id: i32,
    pub indexer: String,
    /// 1 (best) to 50. Lower number wins.
    pub indexer_priority: i32,
    pub protocol: DownloadProtocol,
    pub publish_date: DateTime<Utc>,
    pub container: Option<String>,
    pub indexer_flags: IndexerFlags,
    pub imdb_id: Option<String>,
    pub tmdb_id: i32,
    pub torrent: Option<TorrentRelease>,
}

impl ReleaseInfo {
    pub fn age_minutes(&self, now: DateTime<Utc>) -> f64 {
        (now - self.publish_date).num_milliseconds() as f64 / 60_000.0
    }

    pub fn age_hours(&self, now: DateTime<Utc>) -> f64 {
        (now - self.publish_date).num_milliseconds() as f64 / 3_600_000.0
    }

    /// Whole days, truncated (.NET `TimeSpan.Days`).
    pub fn age_days(&self, now: DateTime<Utc>) -> i64 {
        (now - self.publish_date).num_days()
    }

    pub fn seeders(&self) -> Option<i32> {
        self.torrent.as_ref().and_then(|t| t.seeders)
    }

    pub fn peers(&self) -> Option<i32> {
        self.torrent.as_ref().and_then(|t| t.peers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages() {
        use chrono::{Duration, TimeZone};
        let now = Utc.with_ymd_and_hms(2026, 1, 10, 12, 0, 0).unwrap();
        let r = ReleaseInfo {
            guid: String::new(),
            title: String::new(),
            size: 0,
            download_url: String::new(),
            indexer_id: 1,
            indexer: String::new(),
            indexer_priority: 25,
            protocol: DownloadProtocol::Usenet,
            publish_date: now - Duration::hours(30),
            container: None,
            indexer_flags: IndexerFlags::NONE,
            imdb_id: None,
            tmdb_id: 0,
            torrent: None,
        };
        assert_eq!(r.age_hours(now), 30.0);
        assert_eq!(r.age_minutes(now), 1800.0);
        assert_eq!(r.age_days(now), 1, "truncated, like TimeSpan.Days");
        assert_eq!(r.seeders(), None);
    }

    #[test]
    fn has_flag_semantics() {
        let f = IndexerFlags::G_FREELEECH | IndexerFlags::G_SCENE;
        assert!(f.contains(IndexerFlags::G_SCENE));
        assert!(f.contains(IndexerFlags::G_FREELEECH));
        assert!(!f.contains(IndexerFlags::NUKED));
        // HasFlag(0) is always true, as in .NET.
        assert!(IndexerFlags::NONE.contains(IndexerFlags::NONE));
    }
}
