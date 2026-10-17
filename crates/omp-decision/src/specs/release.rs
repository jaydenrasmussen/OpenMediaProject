//! Checks on the release itself and its fit with the movie and settings.

use std::sync::LazyLock;

use omp_core::{
    language::Language as Lang,
    release::{DownloadProtocol, IndexerFlags},
};
use omp_parser::re::Re;
use omp_quality::release_profile::term_matches;

use super::{Specification, megabytes, size_suffix};
use crate::{
    model::{DecisionData, DecisionInfo, Subject},
    reason::{RejectionKind, RejectionReason as R, Verdict},
};

/// Runtime assumed when the provider has none.
const DEFAULT_RUNTIME_MINUTES: i64 = 110;

pub struct AcceptableSize;

impl Specification for AcceptableSize {
    fn name(&self) -> &'static str {
        "AcceptableSize"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        let size = s.release.size as i64;
        if size == 0 {
            return Verdict::Accept;
        }
        let Some(definition) = env.quality_definitions().get(s.parsed.quality.quality) else {
            return Verdict::Accept;
        };
        let runtime = match s.metadata.runtime {
            0 => DEFAULT_RUNTIME_MINUTES,
            r => i64::from(r),
        };

        if let Some(min) = definition.min_size {
            let min_size = megabytes(min) * runtime;
            if size < min_size {
                return Verdict::reject(
                    R::BelowMinimumSize,
                    format!(
                        "{} is smaller than minimum allowed {} (for {})",
                        size_suffix(size),
                        size_suffix(min_size),
                        s.movie.title
                    ),
                );
            }
        }
        match definition.max_size {
            None => {}
            Some(0.0) => {}
            Some(max) => {
                let max_size = megabytes(max) * runtime;
                if size > max_size {
                    return Verdict::reject(
                        R::AboveMaximumSize,
                        format!(
                            "{} is larger than maximum allowed {} (for {})",
                            size_suffix(size),
                            size_suffix(max_size),
                            s.movie.title
                        ),
                    );
                }
            }
        }
        Verdict::Accept
    }
}

pub struct MaximumSize;

impl Specification for MaximumSize {
    fn name(&self) -> &'static str {
        "MaximumSize"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        let maximum = env.config().maximum_size * 1024 * 1024;
        let size = s.release.size as i64;
        if maximum == 0 || size == 0 {
            return Verdict::Accept;
        }
        if size > maximum {
            return Verdict::reject(
                R::MaximumSizeExceeded,
                format!(
                    "{} is too big, maximum size is {} (Settings->Indexers->Maximum Size)",
                    size_suffix(size),
                    size_suffix(maximum)
                ),
            );
        }
        Verdict::Accept
    }
}

pub struct MinimumAge;

impl Specification for MinimumAge {
    fn name(&self) -> &'static str {
        "MinimumAge"
    }

    fn kind(&self) -> RejectionKind {
        RejectionKind::Temporary
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        if s.release.protocol != DownloadProtocol::Usenet {
            return Verdict::Accept;
        }
        let minimum = env.config().minimum_age;
        if minimum == 0 {
            return Verdict::Accept;
        }
        let age = s.release.age_minutes(env.now());
        if age < f64::from(minimum) {
            return Verdict::reject(
                R::MinimumAge,
                format!("Only {age:.1} minutes old, minimum age is {minimum} minutes"),
            );
        }
        Verdict::Accept
    }
}

pub struct Retention;

impl Specification for Retention {
    fn name(&self) -> &'static str {
        "Retention"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        if s.release.protocol != DownloadProtocol::Usenet {
            return Verdict::Accept;
        }
        let retention = i64::from(env.config().retention);
        if retention > 0 && s.release.age_days(env.now()) > retention {
            return Verdict::reject(R::MaximumAge, "Older than configured retention");
        }
        Verdict::Accept
    }
}

pub struct Protocol;

impl Specification for Protocol {
    fn name(&self) -> &'static str {
        "Protocol"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        let profile = env.delay_profile_for(&s.movie.tags);
        match s.release.protocol {
            DownloadProtocol::Usenet if !profile.enable_usenet => {
                Verdict::reject(R::ProtocolDisabled, "Usenet is not enabled for this movie")
            }
            DownloadProtocol::Torrent if !profile.enable_torrent => {
                Verdict::reject(R::ProtocolDisabled, "Torrent is not enabled for this movie")
            }
            _ => Verdict::Accept,
        }
    }
}

pub struct NotSample;

impl Specification for NotSample {
    fn name(&self) -> &'static str {
        "NotSample"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, _: &dyn DecisionData) -> Verdict {
        if s.release.title.to_lowercase().contains("sample") && s.release.size < 70 * 1024 * 1024 {
            return Verdict::reject(R::Sample, "Sample");
        }
        Verdict::Accept
    }
}

static DISC_PATTERNS: LazyLock<[Re; 3]> = LazyLock::new(|| {
    [
        Re::ci(
            r"(?:dis[ck])(?:[-_. ]\d+[-_. ])(?:(?:(?:480|720|1080|2160)[ip]|)[-_. ])?(?:Blu\-?ray)",
        ),
        Re::ci(r"(?:(?:480|720|1080|2160)[ip]|)[-_. ](?:full)[-_. ](?:Blu\-?ray)"),
        Re::ci(r"(?:\d?x?M?DVD-?[R59])(?:[ ._]|$)"),
    ]
});

pub struct RawDisk;

impl Specification for RawDisk {
    fn name(&self) -> &'static str {
        "RawDisk"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, _: &dyn DecisionData) -> Verdict {
        if DISC_PATTERNS.iter().any(|r| r.is_match(&s.release.title)) {
            return Verdict::reject(R::Raw, "Raw Bluray/DVD release");
        }
        match s
            .release
            .container
            .as_deref()
            .map(str::to_lowercase)
            .as_deref()
        {
            Some("vob" | "iso") => Verdict::reject(R::Raw, "Raw DVD release"),
            Some("m2ts") => Verdict::reject(R::Raw, "Raw Bluray release"),
            _ => Verdict::Accept,
        }
    }
}

pub struct Language;

impl Specification for Language {
    fn name(&self) -> &'static str {
        "Language"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, _: &dyn DecisionData) -> Verdict {
        let wanted = s.profile.language;
        if wanted == Lang::ANY {
            return Verdict::Accept;
        }
        let found = || {
            s.languages
                .iter()
                .map(|l| l.name)
                .collect::<Vec<_>>()
                .join(", ")
        };
        if wanted == Lang::ORIGINAL {
            let original =
                Lang::from_iso639_1(&s.metadata.original_language).unwrap_or(Lang::UNKNOWN);
            if !s.languages.contains(&original) {
                return Verdict::reject(
                    R::WantedLanguage,
                    format!(
                        "Original Language ({original}) is wanted, but found {}",
                        found()
                    ),
                );
            }
            return Verdict::Accept;
        }
        if !s.languages.contains(&wanted) {
            return Verdict::reject(
                R::WantedLanguage,
                format!("{wanted} is wanted, but found {}", found()),
            );
        }
        Verdict::Accept
    }
}

pub struct HardcodeSubs;

impl Specification for HardcodeSubs {
    fn name(&self) -> &'static str {
        "HardcodeSubs"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        let config = env.config();
        let Some(subs) = s
            .parsed
            .hardcoded_subs
            .as_deref()
            .filter(|h| !h.trim().is_empty())
        else {
            return Verdict::Accept;
        };
        if config.allow_hardcoded_subs {
            return Verdict::Accept;
        }
        let lower = subs.to_lowercase();
        let whitelisted = config
            .whitelisted_hardcoded_subs
            .split(',')
            .any(|t| !t.trim().is_empty() && lower.contains(&t.to_lowercase()));
        if whitelisted {
            Verdict::Accept
        } else {
            Verdict::reject(R::HardcodeSubtitles, format!("Hardcode subs found: {subs}"))
        }
    }
}

pub struct ReleaseRestrictions;

impl Specification for ReleaseRestrictions {
    fn name(&self) -> &'static str {
        "ReleaseRestrictions"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        let title = &s.release.title;
        let profiles = env.release_profiles(&s.movie.tags, s.release.indexer_id);

        let contains_any = |terms: &[String]| -> Result<Vec<String>, String> {
            let mut found = Vec::new();
            for term in terms {
                if term_matches(term, title)? {
                    found.push(term.clone());
                }
            }
            Ok(found)
        };

        for p in profiles.iter().filter(|p| !p.required.is_empty()) {
            match contains_any(&p.required) {
                Err(e) => {
                    return Verdict::reject(R::DecisionError, format!("ReleaseRestrictions: {e}"));
                }
                Ok(found) if found.is_empty() => {
                    return Verdict::reject(
                        R::MustContainMissing,
                        format!(
                            "Does not contain one of the required terms: {}",
                            p.required.join(", ")
                        ),
                    );
                }
                Ok(_) => {}
            }
        }
        for p in profiles.iter().filter(|p| !p.ignored.is_empty()) {
            match contains_any(&p.ignored) {
                Err(e) => {
                    return Verdict::reject(R::DecisionError, format!("ReleaseRestrictions: {e}"));
                }
                Ok(found) if !found.is_empty() => {
                    return Verdict::reject(
                        R::MustNotContainPresent,
                        format!("Contains these ignored terms: {}", found.join(", ")),
                    );
                }
                Ok(_) => {}
            }
        }
        Verdict::Accept
    }
}

pub struct RequiredIndexerFlags;

impl Specification for RequiredIndexerFlags {
    fn name(&self) -> &'static str {
        "RequiredIndexerFlags"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        let Some(torrent) = env.indexer(s.release.indexer_id).and_then(|i| i.torrent) else {
            return Verdict::Accept;
        };
        if torrent.required_flags.is_empty() {
            return Verdict::Accept;
        }
        let mut required = IndexerFlags::NONE;
        for flag in &torrent.required_flags {
            if s.release.indexer_flags.contains(*flag) {
                return Verdict::Accept;
            }
            required = required | *flag;
        }
        Verdict::reject(
            R::RequiredFlags,
            format!(
                "None of the required indexer flags {} where found. Found flags: {}",
                required.bits(),
                s.release.indexer_flags.bits()
            ),
        )
    }
}

pub struct TorrentSeeding;

impl Specification for TorrentSeeding {
    fn name(&self) -> &'static str {
        "TorrentSeeding"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        if s.release.torrent.is_none() || s.release.indexer_id == 0 {
            return Verdict::Accept;
        }
        let Some(settings) = env.indexer(s.release.indexer_id).and_then(|i| i.torrent) else {
            return Verdict::Accept;
        };
        match s.release.seeders() {
            Some(seeders) if seeders < settings.minimum_seeders => Verdict::reject(
                R::MinimumSeeders,
                format!(
                    "Not enough seeders: {seeders}. Minimum seeders: {}",
                    settings.minimum_seeders
                ),
            ),
            _ => Verdict::Accept,
        }
    }
}

pub struct IndexerTag;

impl Specification for IndexerTag {
    fn name(&self) -> &'static str {
        "IndexerTag"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        if s.release.indexer_id == 0 {
            return Verdict::Accept;
        }
        let Some(indexer) = env.indexer(s.release.indexer_id) else {
            return Verdict::Accept;
        };
        if !indexer.tags.is_empty() && !indexer.tags.iter().any(|t| s.movie.tags.contains(t)) {
            return Verdict::reject(
                R::NoMatchingTag,
                "Movie tags do not match any of the indexer tags",
            );
        }
        Verdict::Accept
    }
}

pub struct MonitoredMovie;

impl Specification for MonitoredMovie {
    fn name(&self) -> &'static str {
        "MonitoredMovie"
    }

    fn evaluate(&self, s: &Subject<'_>, info: &DecisionInfo, _: &dyn DecisionData) -> Verdict {
        if info.user_invoked_search() {
            return Verdict::Accept;
        }
        if !s.movie.monitored {
            return Verdict::reject(R::MovieNotMonitored, "Movie is not monitored");
        }
        Verdict::Accept
    }
}

pub struct Availability;

impl Specification for Availability {
    fn name(&self) -> &'static str {
        "Availability"
    }

    fn evaluate(&self, s: &Subject<'_>, info: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        if info.user_invoked_search() {
            return Verdict::Accept;
        }
        let delay = env.config().availability_delay;
        if !s
            .metadata
            .is_available(s.movie.minimum_availability, delay, env.now())
        {
            return Verdict::reject(
                R::Availability,
                format!(
                    "Movie {} will only be considered available {delay} days after {:?}",
                    s.movie.title, s.movie.minimum_availability
                ),
            );
        }
        Verdict::Accept
    }
}

pub struct SearchMovie;

impl Specification for SearchMovie {
    fn name(&self) -> &'static str {
        "SearchMovie"
    }

    fn evaluate(&self, s: &Subject<'_>, info: &DecisionInfo, _: &dyn DecisionData) -> Verdict {
        match info.search {
            Some(search) if search.movie_id != s.movie.id => {
                Verdict::reject(R::WrongMovie, "Wrong movie")
            }
            _ => Verdict::Accept,
        }
    }
}

pub struct Blocklist;

impl Specification for Blocklist {
    fn name(&self) -> &'static str {
        "Blocklist"
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        if env.is_blocklisted(s.movie.id, s.release) {
            return Verdict::reject(R::Blocklisted, "Release is blocklisted");
        }
        Verdict::Accept
    }
}

pub struct BlockedIndexer;

impl Specification for BlockedIndexer {
    fn name(&self) -> &'static str {
        "BlockedIndexer"
    }

    fn kind(&self) -> RejectionKind {
        RejectionKind::Temporary
    }

    fn evaluate(&self, s: &Subject<'_>, _: &DecisionInfo, env: &dyn DecisionData) -> Verdict {
        if let Some(till) = env.indexer_blocked_till(s.release.indexer_id) {
            return Verdict::reject(
                R::IndexerDisabled,
                format!(
                    "Indexer {} is blocked till {till} due to failures, cannot grab release.",
                    s.release.indexer
                ),
            );
        }
        Verdict::Accept
    }
}
