//! History event types, with Radarr's numeric values so they are stable on disk.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HistoryEvent {
    Unknown,
    Grabbed,
    DownloadFolderImported,
    DownloadFailed,
    MovieFileDeleted,
    MovieFolderImported,
    MovieFileRenamed,
    DownloadIgnored,
}

impl HistoryEvent {
    pub fn as_i32(self) -> i32 {
        match self {
            HistoryEvent::Unknown => 0,
            HistoryEvent::Grabbed => 1,
            HistoryEvent::DownloadFolderImported => 3,
            HistoryEvent::DownloadFailed => 4,
            HistoryEvent::MovieFileDeleted => 6,
            HistoryEvent::MovieFolderImported => 7,
            HistoryEvent::MovieFileRenamed => 8,
            HistoryEvent::DownloadIgnored => 9,
        }
    }

    /// Unknown values map to `Unknown` rather than failing, so a newer
    /// database can still be read.
    pub fn from_i32(value: i32) -> HistoryEvent {
        match value {
            1 => HistoryEvent::Grabbed,
            3 => HistoryEvent::DownloadFolderImported,
            4 => HistoryEvent::DownloadFailed,
            6 => HistoryEvent::MovieFileDeleted,
            7 => HistoryEvent::MovieFolderImported,
            8 => HistoryEvent::MovieFileRenamed,
            9 => HistoryEvent::DownloadIgnored,
            _ => HistoryEvent::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_and_unknowns_are_tolerated() {
        for e in [
            HistoryEvent::Grabbed,
            HistoryEvent::DownloadFolderImported,
            HistoryEvent::DownloadFailed,
            HistoryEvent::MovieFileDeleted,
            HistoryEvent::MovieFolderImported,
            HistoryEvent::MovieFileRenamed,
            HistoryEvent::DownloadIgnored,
        ] {
            assert_eq!(HistoryEvent::from_i32(e.as_i32()), e);
        }
        assert_eq!(
            HistoryEvent::from_i32(2),
            HistoryEvent::Unknown,
            "removed upstream"
        );
        assert_eq!(HistoryEvent::from_i32(999), HistoryEvent::Unknown);
    }
}
