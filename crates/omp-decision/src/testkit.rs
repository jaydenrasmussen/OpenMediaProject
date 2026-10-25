//! Fixtures for the spec tests: an owned set of inputs and a fake environment.

use chrono::{DateTime, Duration, TimeZone, Utc};
use omp_core::{
    language::Language,
    library::{ExistingFile, LibraryMovie},
    movie::{MovieMetadata, MovieStatus},
    quality::{Quality, QualityModel},
    release::{DownloadProtocol, IndexerFlags, ReleaseInfo},
};
use omp_parser::title::ParsedMovieInfo;
use omp_quality::{
    definition::QualityDefinitions,
    delay::DelayProfile,
    profile::{ProfileItem, QualityProfile},
    release_profile::ReleaseProfile,
};

use crate::model::*;

pub fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap()
}

pub fn model(quality: Quality) -> QualityModel {
    QualityModel {
        quality,
        ..Default::default()
    }
}

/// SDTV (off), DVD, WEB 720p, WEB 1080p, Bluray 1080p; cutoff WEB 1080p.
pub fn profile() -> QualityProfile {
    let q = |quality, allowed| ProfileItem::Quality { quality, allowed };
    QualityProfile {
        id: 1,
        name: "HD".into(),
        cutoff: Quality::WEBDL_1080P.id,
        items: vec![
            q(Quality::SDTV, false),
            q(Quality::DVD, true),
            q(Quality::WEBDL_720P, true),
            q(Quality::WEBDL_1080P, true),
            q(Quality::BLURAY_1080P, true),
        ],
        min_format_score: 0,
        cutoff_format_score: 0,
        min_upgrade_format_score: 1,
        format_items: vec![],
        language: Language::ANY,
        upgrade_allowed: true,
    }
}

/// Everything a [`Subject`] borrows, with sensible defaults: a 4 GB WEB-DL
/// 1080p usenet release, an hour old, for a monitored movie we have no file for.
pub struct Fx {
    pub release: ReleaseInfo,
    pub parsed: ParsedMovieInfo,
    pub movie: LibraryMovie,
    pub metadata: MovieMetadata,
    pub profile: QualityProfile,
    pub existing_file: Option<ExistingFile>,
    pub format_ids: Vec<i32>,
    pub score: i32,
    pub languages: Vec<Language>,
    pub source: ReleaseSource,
}

impl Fx {
    pub fn new() -> Self {
        let release = ReleaseInfo {
            guid: "g".into(),
            title: "Some.Movie.2020.1080p.WEB-DL.x264-GRP".into(),
            size: 4 * 1024 * 1024 * 1024,
            download_url: "https://example.invalid/x".into(),
            indexer_id: 1,
            indexer: "Idx".into(),
            indexer_priority: 25,
            protocol: DownloadProtocol::Usenet,
            publish_date: now() - Duration::hours(1),
            container: None,
            indexer_flags: IndexerFlags::NONE,
            imdb_id: None,
            tmdb_id: 0,
            torrent: None,
        };
        Self {
            release,
            parsed: ParsedMovieInfo {
                movie_titles: vec!["Some Movie".into()],
                year: 2020,
                quality: model(Quality::WEBDL_1080P),
                release_group: Some("GRP".into()),
                ..Default::default()
            },
            movie: LibraryMovie {
                id: 1,
                tmdb_id: 100,
                title: "Some Movie".into(),
                year: 2020,
                monitored: true,
                minimum_availability: MovieStatus::Announced,
                quality_profile_id: 1,
                path: "/movies/Some Movie (2020)".into(),
                tags: vec![],
            },
            metadata: MovieMetadata {
                tmdb_id: 100,
                title: "Some Movie".into(),
                year: 2020,
                runtime: 120,
                original_language: "en".into(),
                ..Default::default()
            },
            profile: profile(),
            existing_file: None,
            format_ids: vec![],
            score: 0,
            languages: vec![Language::ENGLISH],
            source: ReleaseSource::Rss,
        }
    }

    pub fn subject(&self) -> Subject<'_> {
        Subject {
            release: &self.release,
            parsed: &self.parsed,
            movie: &self.movie,
            metadata: &self.metadata,
            profile: &self.profile,
            existing_file: self.existing_file.as_ref(),
            custom_format_ids: &self.format_ids,
            custom_format_score: self.score,
            languages: &self.languages,
            source: self.source,
        }
    }

    pub fn with_file(mut self, quality: Quality) -> Self {
        self.existing_file = Some(ExistingFile {
            quality: model(quality),
            size: 1,
            release_group: Some("GRP".into()),
            date_added: now() - Duration::days(1),
            custom_format_ids: vec![],
        });
        self
    }

    pub fn quality(mut self, quality: Quality) -> Self {
        self.parsed.quality = model(quality);
        self
    }
}

pub struct Env {
    pub config: DecisionConfig,
    pub definitions: QualityDefinitions,
    pub delay_profiles: Vec<DelayProfile>,
    pub blocklisted: bool,
    pub blocked_till: Option<DateTime<Utc>>,
    pub indexers: Vec<IndexerInfo>,
    pub release_profiles: Vec<ReleaseProfile>,
    pub history: Vec<HistoryEntry>,
    pub queue: Vec<QueueEntry>,
    pub pending: bool,
    pub oldest_pending_minutes: Option<f64>,
    pub free_space: Option<u64>,
}

impl Env {
    pub fn new() -> Self {
        Self {
            config: DecisionConfig::default(),
            definitions: QualityDefinitions::defaults(),
            delay_profiles: vec![DelayProfile::default_profile()],
            blocklisted: false,
            blocked_till: None,
            indexers: vec![],
            release_profiles: vec![],
            history: vec![],
            queue: vec![],
            pending: false,
            oldest_pending_minutes: None,
            free_space: None,
        }
    }
}

impl DecisionData for Env {
    fn now(&self) -> DateTime<Utc> {
        now()
    }

    fn config(&self) -> &DecisionConfig {
        &self.config
    }

    fn quality_definitions(&self) -> &QualityDefinitions {
        &self.definitions
    }

    fn delay_profile_for(&self, tags: &[i32]) -> &DelayProfile {
        omp_quality::delay::best_for_tags(&self.delay_profiles, tags)
            .expect("a default delay profile")
    }

    fn is_blocklisted(&self, _: i64, _: &ReleaseInfo) -> bool {
        self.blocklisted
    }

    fn indexer_blocked_till(&self, _: i32) -> Option<DateTime<Utc>> {
        self.blocked_till
    }

    fn indexer(&self, id: i32) -> Option<IndexerInfo> {
        self.indexers.iter().find(|i| i.id == id).cloned()
    }

    fn release_profiles(&self, tags: &[i32], indexer_id: i32) -> Vec<ReleaseProfile> {
        ReleaseProfile::enabled_for(&self.release_profiles, tags, indexer_id)
            .into_iter()
            .cloned()
            .collect()
    }

    fn history_for_movie(&self, _: i64) -> Vec<HistoryEntry> {
        self.history.clone()
    }

    fn queue_for_movie(&self, _: i64) -> Vec<QueueEntry> {
        self.queue.clone()
    }

    fn movie_has_pending_release(&self, _: i64) -> bool {
        self.pending
    }

    fn oldest_pending_release_age_minutes(&self, _: i64) -> Option<f64> {
        self.oldest_pending_minutes
    }

    fn free_space(&self, _: &str) -> Option<u64> {
        self.free_space
    }
}
