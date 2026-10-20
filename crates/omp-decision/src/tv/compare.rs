//! Ranking approved releases for the same episodes (Sonarr `DownloadDecisionComparer`).

use std::cmp::Ordering;

use omp_core::{release::DownloadProtocol, tv::SeriesType};
use omp_quality::upgrade::ProperDownloadTypes;

use super::model::{EpisodeSubject, TvDecisionData};
use crate::specs::megabytes;

const SIZE_STEP: i64 = 200 * 1024 * 1024;

/// `Greater` means `a` is the better release.
pub fn compare(
    a: &EpisodeSubject<'_>,
    b: &EpisodeSubject<'_>,
    env: &dyn TvDecisionData,
) -> Ordering {
    quality(a, b, env)
        .then_with(|| a.custom_format_score.cmp(&b.custom_format_score))
        .then_with(|| protocol(a, b, env))
        .then_with(|| episode_count(a, b))
        .then_with(|| episode_number(a, b))
        .then_with(|| b.release.indexer_priority.cmp(&a.release.indexer_priority))
        .then_with(|| peers(a, b))
        .then_with(|| age(a, b, env))
        .then_with(|| size(a, b, env))
}

pub fn sort_best_first(subjects: &mut [&EpisodeSubject<'_>], env: &dyn TvDecisionData) {
    subjects.sort_by(|a, b| compare(b, a, env));
}

fn quality(a: &EpisodeSubject<'_>, b: &EpisodeSubject<'_>, env: &dyn TvDecisionData) -> Ordering {
    let index = |s: &EpisodeSubject<'_>| s.profile.index_of(s.parsed.quality.quality, false).index;
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

fn protocol(a: &EpisodeSubject<'_>, b: &EpisodeSubject<'_>, env: &dyn TvDecisionData) -> Ordering {
    let prefers = |s: &EpisodeSubject<'_>| {
        s.release.protocol == env.delay_profile_for(&s.series.tags).preferred_protocol
    };
    prefers(a).cmp(&prefers(b))
}

/// Season packs first; otherwise fewer episodes (more for anime).
fn episode_count(a: &EpisodeSubject<'_>, b: &EpisodeSubject<'_>) -> Ordering {
    a.parsed
        .full_season
        .cmp(&b.parsed.full_season)
        .then_with(|| {
            let by_count = a.episodes.len().cmp(&b.episodes.len());
            if a.series.series_type == SeriesType::Anime
                && b.series.series_type == SeriesType::Anime
            {
                by_count
            } else {
                by_count.reverse()
            }
        })
}

fn episode_number(a: &EpisodeSubject<'_>, b: &EpisodeSubject<'_>) -> Ordering {
    let first = |s: &EpisodeSubject<'_>| {
        s.episodes
            .iter()
            .map(|e| e.episode_number)
            .min()
            .unwrap_or(0)
    };
    first(a).cmp(&first(b)).reverse()
}

fn bucket(n: Option<i32>) -> f64 {
    match n {
        Some(n) if n > 0 => f64::from(n).log10().round_ties_even(),
        _ => 0.0,
    }
}

fn peers(a: &EpisodeSubject<'_>, b: &EpisodeSubject<'_>) -> Ordering {
    if a.release.protocol != DownloadProtocol::Torrent
        || b.release.protocol != DownloadProtocol::Torrent
    {
        return Ordering::Equal;
    }
    bucket(a.release.seeders())
        .total_cmp(&bucket(b.release.seeders()))
        .then_with(|| bucket(a.release.peers()).total_cmp(&bucket(b.release.peers())))
}

fn age(a: &EpisodeSubject<'_>, b: &EpisodeSubject<'_>, env: &dyn TvDecisionData) -> Ordering {
    if a.release.protocol != DownloadProtocol::Usenet
        || b.release.protocol != DownloadProtocol::Usenet
    {
        return Ordering::Equal;
    }
    let score = |s: &EpisodeSubject<'_>| -> f64 {
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

fn runtime(s: &EpisodeSubject<'_>) -> i64 {
    if s.episodes.is_empty() {
        return i64::from(s.series.runtime);
    }
    s.episodes
        .iter()
        .map(|e| {
            if e.runtime > 0 {
                i64::from(e.runtime)
            } else {
                i64::from(s.series.runtime)
            }
        })
        .sum()
}

fn truncate_to(n: i64, level: i64) -> i64 {
    (n / level) * level
}

fn size(a: &EpisodeSubject<'_>, b: &EpisodeSubject<'_>, env: &dyn TvDecisionData) -> Ordering {
    let score = |s: &EpisodeSubject<'_>| -> i64 {
        let preferred = env
            .quality_definitions()
            .get(s.parsed.quality.quality)
            .and_then(|d| d.preferred_size);
        let release_size = s.release.size as i64;
        let rt = runtime(s);
        match preferred {
            Some(p) if rt > 0 => -truncate_to(release_size - rt * megabytes(p), SIZE_STEP).abs(),
            _ => truncate_to(release_size, SIZE_STEP),
        }
    };
    score(a).cmp(&score(b))
}
