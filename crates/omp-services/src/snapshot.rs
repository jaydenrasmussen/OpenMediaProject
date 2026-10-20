use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Utc};
use omp_core::{
    blocklist::{BlocklistEntry, is_blocklisted},
    release::{IndexerFlags, ReleaseInfo},
};
use omp_db::{
    Db,
    repo::{MovieRecord, PendingRelease, ProviderKind},
};
use omp_decision::model::{
    DecisionConfig, DecisionData, HistoryEntry, IndexerInfo, QueueEntry, TorrentIndexerSettings,
};
use omp_indexers::IndexerSettings;
use omp_quality::{
    custom_format::CustomFormat,
    definition::QualityDefinitions,
    delay::{DelayProfile, best_for_tags},
    release_profile::ReleaseProfile,
};

use crate::{
    DiskSpace,
    evaluate::ServiceError,
    formats::{input_for_history, matching_ids},
};

/// Entries in the download queue, by movie id.
pub type QueueMap = HashMap<i64, Vec<QueueEntry>>;

/// Everything the (synchronous) decision engine asks for, loaded up front for
/// a batch of movies so the engine itself never waits on the database.
pub struct DecisionSnapshot {
    now: DateTime<Utc>,
    config: DecisionConfig,
    definitions: QualityDefinitions,
    delay_profiles: Vec<DelayProfile>,
    fallback_delay: DelayProfile,
    release_profiles: Vec<ReleaseProfile>,
    indexers: HashMap<i32, IndexerInfo>,
    blocked_indexers: HashMap<i32, DateTime<Utc>>,
    blocklist: HashMap<i64, Vec<BlocklistEntry>>,
    history: HashMap<i64, Vec<HistoryEntry>>,
    queue: QueueMap,
    pending: HashMap<i64, Vec<PendingRelease>>,
    disk: Arc<dyn DiskSpace>,
}

fn indexer_info(p: &omp_db::repo::Provider) -> IndexerInfo {
    // Settings that fail to read count as defaults rather than blocking decisions.
    let all: IndexerSettings = serde_json::from_value(p.settings.clone()).unwrap_or_default();
    let torrent = (p.implementation == "torznab").then(|| {
        let s = all.clone();
        TorrentIndexerSettings {
            minimum_seeders: s.minimum_seeders,
            required_flags: s.required_flags.into_iter().map(IndexerFlags).collect(),
        }
    });
    IndexerInfo {
        id: p.id as i32,
        tags: p.tags.clone(),
        torrent,
        season_pack_max_single_episode_age_days: all.season_search_maximum_single_episode_age,
    }
}

impl DecisionSnapshot {
    /// Loads the shared settings and the per-movie data for `movies`.
    pub async fn load(
        db: &Db,
        config: DecisionConfig,
        now: DateTime<Utc>,
        movies: &[MovieRecord],
        formats: &[CustomFormat],
        queue: QueueMap,
        disk: Arc<dyn DiskSpace>,
    ) -> Result<Self, ServiceError> {
        let indexer_rows = db.providers().list(ProviderKind::Indexer).await?;
        let blocked_indexers = db
            .providers()
            .blocked(ProviderKind::Indexer, now)
            .await?
            .into_iter()
            .map(|(id, till)| (id as i32, till))
            .collect();

        let mut blocklist = HashMap::new();
        let mut history = HashMap::new();
        let mut pending = HashMap::new();
        for movie in movies {
            let id = movie.movie.id;
            blocklist.insert(id, db.blocklist().for_movie(id).await?);
            pending.insert(id, db.pending_releases().for_movie(id).await?);
            let entries = db
                .history()
                .for_movie(id)
                .await?
                .into_iter()
                .map(|h| HistoryEntry {
                    custom_format_ids: matching_ids(formats, &input_for_history(&h, movie)),
                    event: h.event,
                    date: h.date,
                    quality: h.quality,
                    download_id: h.download_id,
                    source_title: h.source_title,
                })
                .collect();
            history.insert(id, entries);
        }

        Ok(Self {
            now,
            config,
            definitions: db.quality_definitions().load().await?,
            delay_profiles: db.delay_profiles().list().await?,
            fallback_delay: DelayProfile::default_profile(),
            release_profiles: db.release_profiles().list().await?,
            indexers: indexer_rows
                .iter()
                .map(|p| (p.id as i32, indexer_info(p)))
                .collect(),
            blocked_indexers,
            blocklist,
            history,
            queue,
            pending,
            disk,
        })
    }
}

impl DecisionData for DecisionSnapshot {
    fn now(&self) -> DateTime<Utc> {
        self.now
    }

    fn config(&self) -> &DecisionConfig {
        &self.config
    }

    fn quality_definitions(&self) -> &QualityDefinitions {
        &self.definitions
    }

    fn delay_profile_for(&self, tags: &[i32]) -> &DelayProfile {
        best_for_tags(&self.delay_profiles, tags).unwrap_or(&self.fallback_delay)
    }

    fn is_blocklisted(&self, movie_id: i64, release: &ReleaseInfo) -> bool {
        self.blocklist
            .get(&movie_id)
            .is_some_and(|entries| is_blocklisted(entries, release))
    }

    fn indexer_blocked_till(&self, indexer_id: i32) -> Option<DateTime<Utc>> {
        self.blocked_indexers.get(&indexer_id).copied()
    }

    fn indexer(&self, indexer_id: i32) -> Option<IndexerInfo> {
        self.indexers.get(&indexer_id).cloned()
    }

    fn release_profiles(&self, tags: &[i32], indexer_id: i32) -> Vec<ReleaseProfile> {
        ReleaseProfile::enabled_for(&self.release_profiles, tags, indexer_id)
            .into_iter()
            .cloned()
            .collect()
    }

    fn history_for_movie(&self, movie_id: i64) -> Vec<HistoryEntry> {
        self.history.get(&movie_id).cloned().unwrap_or_default()
    }

    fn queue_for_movie(&self, movie_id: i64) -> Vec<QueueEntry> {
        self.queue.get(&movie_id).cloned().unwrap_or_default()
    }

    fn movie_has_pending_release(&self, movie_id: i64) -> bool {
        self.pending.get(&movie_id).is_some_and(|p| !p.is_empty())
    }

    fn oldest_pending_release_age_minutes(&self, movie_id: i64) -> Option<f64> {
        self.pending
            .get(&movie_id)?
            .iter()
            .map(|p| p.release.age_minutes(self.now))
            .max_by(f64::total_cmp)
    }

    fn free_space(&self, path: &str) -> Option<u64> {
        self.disk.free_bytes(path)
    }
}
