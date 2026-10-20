//! Why a release was rejected. Mirrors Radarr's `DownloadRejectionReason`.

/// Permanent rejections never become acceptable; temporary ones may (a delay
/// elapses, an indexer recovers) and send the release to the pending queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionKind {
    Permanent,
    Temporary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RejectionReason {
    Unknown,
    UnknownMovie,
    UnableToParse,
    Error,
    DecisionError,
    Availability,
    MinimumAgeDelay,
    MinimumAgeDelayPushed,
    MovieNotMonitored,
    HistoryRecentCutoffMet,
    HistoryCdhDisabledCutoffMet,
    HistoryHigherPreference,
    HistoryHigherRevision,
    HistoryCutoffMet,
    HistoryCustomFormatCutoffMet,
    HistoryCustomFormatScore,
    HistoryCustomFormatScoreIncrement,
    HistoryUpgradesNotAllowed,
    NoMatchingTag,
    PropersDisabled,
    ProperForOldFile,
    WrongMovie,
    UnknownRuntime,
    BelowMinimumSize,
    AboveMaximumSize,
    AlreadyImportedSameHash,
    AlreadyImportedSameName,
    IndexerDisabled,
    Blocklisted,
    CustomFormatMinimumScore,
    MinimumFreeSpace,
    HardcodeSubtitles,
    WantedLanguage,
    MaximumSizeExceeded,
    MinimumAge,
    MaximumAge,
    Sample,
    ProtocolDisabled,
    QualityNotWanted,
    QualityUpgradesDisabled,
    QueueHigherPreference,
    QueueHigherRevision,
    QueueCutoffMet,
    QueueCustomFormatCutoffMet,
    QueueCustomFormatScore,
    QueueCustomFormatScoreIncrement,
    QueueUpgradesNotAllowed,
    QueuePropersDisabled,
    Raw,
    MustContainMissing,
    MustNotContainPresent,
    RepackDisabled,
    RepackUnknownReleaseGroup,
    RepackReleaseGroupDoesNotMatch,
    RequiredFlags,
    MinimumSeeders,
    DiskHigherPreference,
    DiskHigherRevision,
    DiskCutoffMet,
    DiskCustomFormatCutoffMet,
    DiskCustomFormatScore,
    DiskCustomFormatScoreIncrement,
    DiskUpgradesNotAllowed,
    UnknownSeries,
    UnknownEpisode,
    MatchesAnotherSeries,
    SeriesNotMonitored,
    EpisodeNotMonitored,
    WrongEpisode,
    WrongSeason,
    WrongSeries,
    FullSeason,
    FullSeasonNotAired,
    MultiSeason,
    SplitEpisode,
    ExistingFileHasMoreEpisodes,
    UnknownReleaseGroup,
    ReleaseGroupDoesNotMatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub reason: RejectionReason,
    pub message: String,
    pub kind: RejectionKind,
}

/// A spec's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Accept,
    Reject {
        reason: RejectionReason,
        message: String,
    },
}

impl Verdict {
    pub fn reject(reason: RejectionReason, message: impl Into<String>) -> Self {
        Verdict::Reject {
            reason,
            message: message.into(),
        }
    }

    pub fn is_accept(&self) -> bool {
        matches!(self, Verdict::Accept)
    }
}

/// The three places an existing item can be compared with a new release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    History,
    Queue,
    Disk,
}

impl Family {
    pub fn label(self) -> &'static str {
        match self {
            Family::History => "grab event in history",
            Family::Queue => "release in queue",
            Family::Disk => "existing file on disk",
        }
    }

    pub fn higher_preference(self) -> RejectionReason {
        match self {
            Family::History => RejectionReason::HistoryHigherPreference,
            Family::Queue => RejectionReason::QueueHigherPreference,
            Family::Disk => RejectionReason::DiskHigherPreference,
        }
    }

    pub fn higher_revision(self) -> RejectionReason {
        match self {
            Family::History => RejectionReason::HistoryHigherRevision,
            Family::Queue => RejectionReason::QueueHigherRevision,
            Family::Disk => RejectionReason::DiskHigherRevision,
        }
    }

    pub fn cutoff_met(self) -> RejectionReason {
        match self {
            Family::History => RejectionReason::HistoryCutoffMet,
            Family::Queue => RejectionReason::QueueCutoffMet,
            Family::Disk => RejectionReason::DiskCutoffMet,
        }
    }

    pub fn custom_format_cutoff_met(self) -> RejectionReason {
        match self {
            Family::History => RejectionReason::HistoryCustomFormatCutoffMet,
            Family::Queue => RejectionReason::QueueCustomFormatCutoffMet,
            Family::Disk => RejectionReason::DiskCustomFormatCutoffMet,
        }
    }

    pub fn custom_format_score(self) -> RejectionReason {
        match self {
            Family::History => RejectionReason::HistoryCustomFormatScore,
            Family::Queue => RejectionReason::QueueCustomFormatScore,
            Family::Disk => RejectionReason::DiskCustomFormatScore,
        }
    }

    pub fn custom_format_increment(self) -> RejectionReason {
        match self {
            Family::History => RejectionReason::HistoryCustomFormatScoreIncrement,
            Family::Queue => RejectionReason::QueueCustomFormatScoreIncrement,
            Family::Disk => RejectionReason::DiskCustomFormatScoreIncrement,
        }
    }

    pub fn upgrades_not_allowed(self) -> RejectionReason {
        match self {
            Family::History => RejectionReason::HistoryUpgradesNotAllowed,
            Family::Queue => RejectionReason::QueueUpgradesNotAllowed,
            Family::Disk => RejectionReason::DiskUpgradesNotAllowed,
        }
    }
}
