//! Ranking approved releases for one movie. Ported from Radarr
//! `DownloadDecisionComparer` (GPL-3.0). The first difference wins:
//! quality, custom-format score, preferred protocol, indexer priority,
//! indexer flags, seeders/peers (torrent), age (usenet), size.

use std::cmp::Ordering;

use omp_core::release::{DownloadProtocol, IndexerFlags};
use omp_quality::upgrade::ProperDownloadTypes;

use crate::{
    model::{DecisionData, Subject},
    specs::megabytes,
};

const SIZE_STEP: i64 = 200 * 1024 * 1024;

/// Orders `a` against `b`; `Greater` means `a` is the better release.
pub fn compare(a: &Subject<'_>, b: &Subject<'_>, env: &dyn DecisionData) -> Ordering {
    compare_quality(a, b, env)
        .then_with(|| a.custom_format_score.cmp(&b.custom_format_score))
        .then_with(|| compare_protocol(a, b, env))
        // A lower indexer priority number is better.
        .then_with(|| b.release.indexer_priority.cmp(&a.release.indexer_priority))
        .then_with(|| compare_indexer_flags(a, b, env))
        .then_with(|| compare_peers(a, b))
        .then_with(|| compare_age(a, b, env))
        .then_with(|| compare_size(a, b, env))
}

/// Sorts best first and keeps the order of equal releases.
pub fn sort_best_first(subjects: &mut [&Subject<'_>], env: &dyn DecisionData) {
    subjects.sort_by(|a, b| compare(b, a, env));
}

fn compare_quality(a: &Subject<'_>, b: &Subject<'_>, env: &dyn DecisionData) -> Ordering {
    let index = |s: &Subject<'_>| s.profile.index_of(s.parsed.quality.quality, false).index;
    let by_index = index(a).cmp(&index(b));
    if env.config().propers_and_repacks == ProperDownloadTypes::DoNotPrefer {
        return by_index;
    }
    by_index.then_with(|| {
        a.parsed
            .quality
            .revision
            .partial_cmp(&b.parsed.quality.revision)
            .unwrap_or(Ordering::Equal)
    })
}

fn compare_protocol(a: &Subject<'_>, b: &Subject<'_>, env: &dyn DecisionData) -> Ordering {
    let prefers = |s: &Subject<'_>| {
        s.release.protocol == env.delay_profile_for(&s.movie.tags).preferred_protocol
    };
    prefers(a).cmp(&prefers(b))
}

fn compare_indexer_flags(a: &Subject<'_>, b: &Subject<'_>, env: &dyn DecisionData) -> Ordering {
    if !env.config().prefer_indexer_flags {
        return Ordering::Equal;
    }
    score_flags(a.release.indexer_flags).cmp(&score_flags(b.release.indexer_flags))
}

fn score_flags(flags: IndexerFlags) -> i32 {
    let mut score = 0;
    for good in [
        IndexerFlags::G_DOUBLE_UPLOAD,
        IndexerFlags::G_FREELEECH,
        IndexerFlags::PTP_APPROVED,
        IndexerFlags::PTP_GOLDEN,
        IndexerFlags::G_INTERNAL,
    ] {
        if flags.contains(good) {
            score += 2;
        }
    }
    if flags.contains(IndexerFlags::G_HALFLEECH) {
        score += 1;
    }
    score
}

/// log10 bucket, so 90 and 110 seeders tie. .NET rounds half to even.
fn log_bucket(n: Option<i32>) -> f64 {
    match n {
        Some(n) if n > 0 => f64::from(n).log10().round_ties_even(),
        _ => 0.0,
    }
}

fn compare_peers(a: &Subject<'_>, b: &Subject<'_>) -> Ordering {
    if a.release.protocol != DownloadProtocol::Torrent
        || b.release.protocol != DownloadProtocol::Torrent
    {
        return Ordering::Equal;
    }
    log_bucket(a.release.seeders())
        .total_cmp(&log_bucket(b.release.seeders()))
        .then_with(|| log_bucket(a.release.peers()).total_cmp(&log_bucket(b.release.peers())))
}

fn compare_age(a: &Subject<'_>, b: &Subject<'_>, env: &dyn DecisionData) -> Ordering {
    if a.release.protocol != DownloadProtocol::Usenet
        || b.release.protocol != DownloadProtocol::Usenet
    {
        return Ordering::Equal;
    }
    let score = |s: &Subject<'_>| -> f64 {
        let now = env.now();
        let hours = s.release.age_hours(now);
        let days = s.release.age_days(now);
        if hours < 1.0 {
            1000.0
        } else if hours <= 24.0 {
            100.0
        } else if days <= 7 {
            10.0
        } else {
            -(days as f64).log10().round_ties_even()
        }
    };
    score(a).total_cmp(&score(b))
}

/// Round toward zero to a multiple of `level` (Radarr's `Round(long, long)`).
fn truncate_to(n: i64, level: i64) -> i64 {
    (n / level) * level
}

fn compare_size(a: &Subject<'_>, b: &Subject<'_>, env: &dyn DecisionData) -> Ordering {
    let score = |s: &Subject<'_>| -> i64 {
        let preferred = env
            .quality_definitions()
            .get(s.parsed.quality.quality)
            .and_then(|d| d.preferred_size);
        let size = s.release.size as i64;
        match preferred {
            // Closest to the preferred size wins.
            Some(p) if s.metadata.runtime > 0 => {
                let target = i64::from(s.metadata.runtime) * megabytes(p);
                -truncate_to(size - target, SIZE_STEP).abs()
            }
            // No preferred size means "unlimited": bigger is better.
            _ => truncate_to(size, SIZE_STEP),
        }
    };
    score(a).cmp(&score(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_is_toward_zero() {
        let mb = 1024 * 1024;
        assert_eq!(truncate_to(450 * mb, 200 * mb), 400 * mb);
        assert_eq!(truncate_to(-450 * mb, 200 * mb), -400 * mb);
        assert_eq!(truncate_to(199 * mb, 200 * mb), 0);
    }

    #[test]
    fn log_buckets_round_half_even() {
        assert_eq!(log_bucket(None), 0.0);
        assert_eq!(log_bucket(Some(0)), 0.0);
        assert_eq!(log_bucket(Some(9)), 1.0);
        assert_eq!(log_bucket(Some(110)), 2.0);
        assert_eq!(log_bucket(Some(90)), 2.0);
        assert_eq!(log_bucket(Some(3)), 0.0, "log10(3)=0.48");
    }

    #[test]
    fn flag_scores() {
        assert_eq!(score_flags(IndexerFlags::NONE), 0);
        assert_eq!(
            score_flags(IndexerFlags::G_FREELEECH | IndexerFlags::G_INTERNAL),
            4
        );
        assert_eq!(score_flags(IndexerFlags::G_HALFLEECH), 1);
        assert_eq!(score_flags(IndexerFlags::G_SCENE), 0);
    }
}
