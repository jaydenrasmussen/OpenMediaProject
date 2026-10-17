//! Release-level types shared by indexers, decisions and downloads.

/// Which kind of downloader a release is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DownloadProtocol {
    #[default]
    Unknown,
    Usenet,
    Torrent,
}

/// Flags an indexer attaches to a release (Radarr `IndexerFlags`, GPL-3.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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

#[cfg(test)]
mod tests {
    use super::*;

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
