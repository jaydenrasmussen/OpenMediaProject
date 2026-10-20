//! Evaluating TV releases: match them to the library, score them, run the
//! TV decision engine. Ported from Sonarr `DownloadDecisionMaker` (GPL-3.0).

use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Utc};
use omp_core::{
    blocklist::is_blocklisted,
    release::{IndexerFlags, ReleaseInfo},
};
use omp_db::{
    Db,
    repo::{
        EpisodeFileRecord, EpisodeRecord, PendingTvRelease, ProviderKind, SeriesRecord,
        TvBlocklistEntry,
    },
};
use omp_decision::{
    Rejection, RejectionKind, RejectionReason, ReleaseSource,
    engine::Decision,
    model::{DecisionConfig, HistoryEntry, IndexerInfo, TorrentIndexerSettings},
    tv::{
        EpisodeFileView, EpisodeSubject, EpisodeView, SeriesView, TvDecisionData, TvDecisionEngine,
        TvDecisionInfo, TvQueueEntry, TvSearchContext, compare,
    },
};
use omp_indexers::IndexerSettings;
use omp_parser::tv::{ParsedEpisodeInfo, parse_season_title, parse_title};
use omp_quality::{
    custom_format::{CustomFormat, CustomFormatInput},
    definition::QualityDefinitions,
    delay::{DelayProfile, best_for_tags},
    profile::QualityProfile,
    release_profile::ReleaseProfile,
};

use crate::{
    DiskSpace,
    config::load_decision_config,
    evaluate::ServiceError,
    formats::matching_ids,
    tv_match::{ReleaseIds, SeriesIndex, match_release},
};

/// What is downloading, by series id.
pub type TvQueueMap = HashMap<i64, Vec<TvQueueEntry>>;

fn scene_file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Custom-format input for a release being considered.
pub fn input_for_episode_release(
    parsed: &ParsedEpisodeInfo,
    release: &ReleaseInfo,
    series: &SeriesRecord,
) -> CustomFormatInput {
    CustomFormatInput {
        quality: Some(parsed.quality.quality),
        edition: None,
        release_group: parsed.release_group.clone(),
        simple_release_title: Some(omp_parser::title::simplify_release_title(
            &parsed.release_title,
        )),
        filename: None,
        parsed_year: 0,
        movie_year: Some(series.info.year),
        movie_original_language: omp_core::language::Language::from_iso639_1(
            &series.info.original_language,
        ),
        size: release.size,
        indexer_flags: release.indexer_flags,
        languages: parsed.languages.clone(),
    }
}

/// Custom-format input for a file we already have.
pub fn input_for_episode_file(
    file: &EpisodeFileRecord,
    series: &SeriesRecord,
) -> CustomFormatInput {
    let title = file
        .scene_name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or(file.original_file_path.as_deref().map(scene_file_name))
        .unwrap_or_else(|| scene_file_name(&file.relative_path));
    CustomFormatInput {
        quality: Some(file.quality.quality),
        edition: None,
        release_group: file.release_group.clone(),
        simple_release_title: Some(omp_parser::title::simplify_release_title(title)),
        filename: Some(scene_file_name(&file.relative_path).to_owned()),
        parsed_year: 0,
        movie_year: Some(series.info.year),
        movie_original_language: omp_core::language::Language::from_iso639_1(
            &series.info.original_language,
        ),
        size: file.size,
        indexer_flags: file.indexer_flags,
        languages: file.languages.clone(),
    }
}

fn history_input(
    h: &omp_db::repo::EpisodeHistoryRecord,
    series: &SeriesRecord,
) -> CustomFormatInput {
    let size = h.data.get("size").and_then(|s| s.parse().ok()).unwrap_or(0);
    let flags = h
        .data
        .get("indexer_flags")
        .and_then(|s| s.parse().ok())
        .map_or(IndexerFlags::NONE, IndexerFlags);
    CustomFormatInput {
        quality: Some(h.quality.quality),
        edition: None,
        release_group: h.data.get("release_group").cloned(),
        simple_release_title: Some(omp_parser::title::simplify_release_title(&h.source_title)),
        filename: None,
        parsed_year: 0,
        movie_year: Some(series.info.year),
        movie_original_language: omp_core::language::Language::from_iso639_1(
            &series.info.original_language,
        ),
        size,
        indexer_flags: flags,
        languages: h.languages.clone(),
    }
}

/// Everything the synchronous TV engine asks for, loaded up front.
pub struct TvSnapshot {
    now: DateTime<Utc>,
    config: DecisionConfig,
    definitions: QualityDefinitions,
    delay_profiles: Vec<DelayProfile>,
    fallback_delay: DelayProfile,
    release_profiles: Vec<ReleaseProfile>,
    indexers: HashMap<i32, IndexerInfo>,
    blocked_indexers: HashMap<i32, DateTime<Utc>>,
    blocklist: HashMap<i64, Vec<TvBlocklistEntry>>,
    history: HashMap<i64, Vec<HistoryEntry>>,
    queue: TvQueueMap,
    pending: HashMap<i64, Vec<PendingTvRelease>>,
    disk: Arc<dyn DiskSpace>,
}

fn indexer_info(p: &omp_db::repo::Provider) -> IndexerInfo {
    let torrent = (p.implementation == "torznab").then(|| {
        let s: IndexerSettings = serde_json::from_value(p.settings.clone()).unwrap_or_default();
        TorrentIndexerSettings {
            minimum_seeders: s.minimum_seeders,
            required_flags: s.required_flags.into_iter().map(IndexerFlags).collect(),
        }
    });
    IndexerInfo {
        id: p.id as i32,
        tags: p.tags.clone(),
        torrent,
    }
}

impl TvSnapshot {
    pub async fn load(
        db: &Db,
        config: DecisionConfig,
        now: DateTime<Utc>,
        series: &[SeriesRecord],
        formats: &[CustomFormat],
        queue: TvQueueMap,
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
        for s in series {
            blocklist.insert(s.id, db.tv_blocklist().for_series(s.id).await?);
            pending.insert(s.id, db.pending_tv_releases().for_series(s.id).await?);
            for h in db.episode_history().for_series(s.id).await? {
                let entry = HistoryEntry {
                    custom_format_ids: matching_ids(formats, &history_input(&h, s)),
                    event: h.event,
                    date: h.date,
                    quality: h.quality,
                    download_id: h.download_id.clone(),
                    source_title: h.source_title.clone(),
                };
                history
                    .entry(h.episode_id)
                    .or_insert_with(Vec::new)
                    .push(entry);
            }
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

impl TvDecisionData for TvSnapshot {
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
    fn is_blocklisted(&self, series_id: i64, release: &ReleaseInfo) -> bool {
        self.blocklist.get(&series_id).is_some_and(|entries| {
            let plain: Vec<_> = entries.iter().map(|e| e.entry.clone()).collect();
            is_blocklisted(&plain, release)
        })
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
    fn history_for_episode(&self, episode_id: i64) -> Vec<HistoryEntry> {
        self.history.get(&episode_id).cloned().unwrap_or_default()
    }
    fn queue_for_series(&self, series_id: i64) -> Vec<TvQueueEntry> {
        self.queue.get(&series_id).cloned().unwrap_or_default()
    }
    fn has_pending_release(&self, series_id: i64, episode_ids: &[i64]) -> bool {
        self.pending.get(&series_id).is_some_and(|p| {
            p.iter()
                .any(|r| r.episode_ids.iter().any(|id| episode_ids.contains(id)))
        })
    }
    fn oldest_pending_age_minutes(&self, series_id: i64, episode_ids: &[i64]) -> Option<f64> {
        self.pending
            .get(&series_id)?
            .iter()
            .filter(|r| r.episode_ids.iter().any(|id| episode_ids.contains(id)))
            .map(|p| p.release.age_minutes(self.now))
            .max_by(f64::total_cmp)
    }
    fn free_space(&self, path: &str) -> Option<u64> {
        self.disk.free_bytes(path)
    }
}

pub struct TvEvaluateRequest {
    pub releases: Vec<ReleaseInfo>,
    pub search: Option<TvSearchContext>,
    pub pushed: bool,
    pub now: DateTime<Utc>,
    pub queue: TvQueueMap,
    pub disk: Arc<dyn DiskSpace>,
}

/// One release with everything learned about it.
#[derive(Debug, Clone)]
pub struct EvaluatedTvRelease {
    pub release: ReleaseInfo,
    pub parsed: Option<ParsedEpisodeInfo>,
    pub series: Option<SeriesRecord>,
    pub episodes: Vec<EpisodeRecord>,
    pub files: Vec<EpisodeFileRecord>,
    pub profile: Option<QualityProfile>,
    pub custom_format_ids: Vec<i32>,
    pub custom_format_score: i32,
    pub source: ReleaseSource,
    pub mapped_season: Option<i32>,
    pub decision: Decision,
    series_view: Option<SeriesView>,
    episode_views: Vec<EpisodeView>,
    file_views: Vec<EpisodeFileView>,
}

impl EvaluatedTvRelease {
    pub fn with_subject<R>(&self, f: impl FnOnce(&EpisodeSubject<'_>) -> R) -> Option<R> {
        let subject = EpisodeSubject {
            release: &self.release,
            parsed: self.parsed.as_ref()?,
            series: self.series_view.as_ref()?,
            episodes: &self.episode_views,
            files: &self.file_views,
            profile: self.profile.as_ref()?,
            custom_format_ids: &self.custom_format_ids,
            custom_format_score: self.custom_format_score,
            source: self.source,
            mapped_season: self.mapped_season,
        };
        Some(f(&subject))
    }

    pub fn episode_ids(&self) -> Vec<i64> {
        self.episodes.iter().map(|e| e.id).collect()
    }
}

pub struct TvEvaluation {
    pub items: Vec<EvaluatedTvRelease>,
    pub snapshot: TvSnapshot,
}

impl TvEvaluation {
    /// Approved and temporarily rejected releases, best first within each
    /// series (series in the order they first appear).
    pub fn qualified_best_first(&self) -> Vec<usize> {
        self.ranked(|it| it.decision.approved() || it.decision.temporarily_rejected())
    }

    pub fn approved_best_first(&self) -> Vec<usize> {
        self.ranked(|it| it.decision.approved())
    }

    fn ranked(&self, include: impl Fn(&EvaluatedTvRelease) -> bool) -> Vec<usize> {
        let mut groups: Vec<(i64, Vec<usize>)> = Vec::new();
        for (i, item) in self.items.iter().enumerate() {
            let Some(series) = item
                .series
                .as_ref()
                .filter(|_| include(item) && !item.episodes.is_empty())
            else {
                continue;
            };
            match groups.iter_mut().find(|(id, _)| *id == series.id) {
                Some((_, v)) => v.push(i),
                None => groups.push((series.id, vec![i])),
            }
        }
        groups
            .into_iter()
            .flat_map(|(_, idx)| self.rank(&idx))
            .collect()
    }

    fn rank(&self, indices: &[usize]) -> Vec<usize> {
        let mut order: Vec<usize> = indices.to_vec();
        order.sort_by(|&a, &b| {
            let (x, y) = (&self.items[a], &self.items[b]);
            match (
                x.with_subject(|sa| y.with_subject(|sb| compare::compare(sb, sa, &self.snapshot)))
                    .flatten(),
                (),
            ) {
                (Some(o), ()) => o,
                _ => std::cmp::Ordering::Equal,
            }
        });
        order
    }
}

fn rejected(reason: RejectionReason, message: impl Into<String>) -> Decision {
    Decision {
        rejections: vec![Rejection {
            reason,
            message: message.into(),
            kind: RejectionKind::Permanent,
        }],
    }
}

fn release_source(search: &Option<TvSearchContext>, pushed: bool) -> ReleaseSource {
    match search {
        None if pushed => ReleaseSource::ReleasePush,
        None => ReleaseSource::Rss,
        Some(s) if s.interactive => ReleaseSource::InteractiveSearch,
        Some(s) if s.user_invoked => ReleaseSource::UserInvokedSearch,
        Some(_) => ReleaseSource::Search,
    }
}

fn series_view(s: &SeriesRecord, episodes: &[EpisodeRecord]) -> SeriesView {
    let first_season = episodes
        .iter()
        .map(|e| e.season_number)
        .filter(|n| *n > 0)
        .min();
    let first_season_start = first_season.and_then(|n| {
        episodes
            .iter()
            .filter(|e| e.season_number == n)
            .filter_map(|e| e.air_date_utc)
            .min()
    });
    SeriesView {
        id: s.id,
        title: s.info.title.clone(),
        series_type: s.series_type,
        monitored: s.monitored,
        tags: s.tags.clone(),
        runtime: s.info.runtime,
        path: s.path.clone(),
        first_season,
        first_season_start,
    }
}

/// Evaluates a batch of releases. RSS-style evaluation (no search) silently
/// drops what it cannot parse; a search reports it as a rejection.
pub async fn evaluate_tv_releases(
    db: &Db,
    engine: &TvDecisionEngine,
    request: TvEvaluateRequest,
) -> Result<TvEvaluation, ServiceError> {
    let TvEvaluateRequest {
        releases,
        search,
        pushed,
        now,
        queue,
        disk,
    } = request;
    let source = release_source(&search, pushed);
    let info = TvDecisionInfo {
        pushed_release: pushed,
        search: search.clone(),
    };

    let formats = db.custom_formats().load_compiled().await?;
    let profiles: HashMap<i32, QualityProfile> = db
        .quality_profiles()
        .list()
        .await?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    let index = SeriesIndex::load(db).await?;
    let target = match &search {
        Some(s) => Some(db.series().get(s.series_id).await?),
        None => None,
    };

    struct Matched {
        release: ReleaseInfo,
        parsed: Option<ParsedEpisodeInfo>,
        series: Option<SeriesRecord>,
        episodes: Vec<EpisodeRecord>,
        mapped_season: Option<i32>,
        early: Option<Decision>,
    }
    let mut matched: Vec<Matched> = Vec::new();
    for release in releases {
        let parsed = parse_title(&release.title)
            .or_else(|| parse_season_title(&release.title))
            .filter(|p| !p.series_title.trim().is_empty());
        let Some(parsed) = parsed else {
            if search.is_some() {
                matched.push(Matched {
                    release,
                    parsed: None,
                    series: None,
                    episodes: vec![],
                    mapped_season: None,
                    early: Some(rejected(
                        RejectionReason::UnableToParse,
                        "Unable to parse release",
                    )),
                });
            }
            continue;
        };
        let ids = ReleaseIds {
            tmdb_id: i64::from(release.tmdb_id),
            imdb_id: release.imdb_id.clone(),
        };
        match match_release(db, &index, &parsed, &ids, target.as_ref()).await? {
            None => matched.push(Matched {
                release,
                parsed: Some(parsed),
                series: None,
                episodes: vec![],
                mapped_season: None,
                early: Some(rejected(RejectionReason::UnknownSeries, "Unknown Series")),
            }),
            Some(m) if m.episodes.is_empty() => matched.push(Matched {
                release,
                parsed: Some(parsed),
                series: Some(m.series),
                episodes: vec![],
                mapped_season: m.mapped_season,
                early: Some(rejected(
                    RejectionReason::UnknownEpisode,
                    "Unable to identify correct episode(s) using release name",
                )),
            }),
            Some(m) => matched.push(Matched {
                release,
                parsed: Some(parsed),
                series: Some(m.series),
                episodes: m.episodes,
                mapped_season: m.mapped_season,
                early: None,
            }),
        }
    }

    // Phase 2: what the engine needs for the series involved.
    let mut involved: Vec<SeriesRecord> = Vec::new();
    for m in &matched {
        if let Some(s) = &m.series
            && !involved.iter().any(|x| x.id == s.id)
        {
            involved.push(s.clone());
        }
    }
    let config = load_decision_config(db).await?;
    let snapshot = TvSnapshot::load(db, config, now, &involved, &formats, queue, disk).await?;

    let mut all_episodes: HashMap<i64, Vec<EpisodeRecord>> = HashMap::new();
    let mut files: HashMap<i64, Vec<EpisodeFileRecord>> = HashMap::new();
    let mut file_formats: HashMap<i64, Vec<i32>> = HashMap::new();
    for s in &involved {
        all_episodes.insert(s.id, db.episodes().for_series(s.id).await?);
        let list = db.episode_files().for_series(s.id).await?;
        for f in &list {
            file_formats.insert(f.id, matching_ids(&formats, &input_for_episode_file(f, s)));
        }
        files.insert(s.id, list);
    }

    // Phase 3: score and decide. No I/O from here on.
    let mut items = Vec::with_capacity(matched.len());
    for m in matched {
        let profile = m
            .series
            .as_ref()
            .and_then(|s| profiles.get(&s.quality_profile_id))
            .cloned();
        let (mut ids, mut score) = (Vec::new(), 0);
        if let (Some(parsed), Some(series), Some(profile)) = (&m.parsed, &m.series, &profile) {
            ids = matching_ids(
                &formats,
                &input_for_episode_release(parsed, &m.release, series),
            );
            score = profile.calculate_custom_format_score(&ids);
        }
        let (series_v, episode_views, file_views, my_files) = match &m.series {
            Some(s) => {
                let eps_all = &all_episodes[&s.id];
                let all_files = &files[&s.id];
                let views: Vec<EpisodeView> = m
                    .episodes
                    .iter()
                    .map(|e| EpisodeView {
                        id: e.id,
                        season_number: e.season_number,
                        episode_number: e.episode_number,
                        runtime: e.runtime,
                        air_date_utc: e.air_date_utc,
                        monitored: e.monitored,
                        file_id: e.episode_file_id,
                    })
                    .collect();
                let mut seen = std::collections::HashSet::new();
                let mut fviews = Vec::new();
                let mut fvals = Vec::new();
                for fid in m.episodes.iter().filter_map(|e| e.episode_file_id) {
                    if !seen.insert(fid) {
                        continue;
                    }
                    if let Some(f) = all_files.iter().find(|f| f.id == fid) {
                        fviews.push(EpisodeFileView {
                            id: f.id,
                            quality: f.quality,
                            release_group: f.release_group.clone(),
                            date_added: f.date_added,
                            custom_format_ids: file_formats.get(&f.id).cloned().unwrap_or_default(),
                            episode_ids: eps_all
                                .iter()
                                .filter(|e| e.episode_file_id == Some(f.id))
                                .map(|e| e.id)
                                .collect(),
                        });
                        fvals.push(f.clone());
                    }
                }
                (Some(series_view(s, eps_all)), views, fviews, fvals)
            }
            None => (None, vec![], vec![], vec![]),
        };
        let mut item = EvaluatedTvRelease {
            release: m.release,
            parsed: m.parsed,
            series: m.series,
            episodes: m.episodes,
            files: my_files,
            profile,
            custom_format_ids: ids,
            custom_format_score: score,
            source,
            mapped_season: m.mapped_season,
            decision: Decision::default(),
            series_view: series_v,
            episode_views,
            file_views,
        };
        item.decision = match m.early {
            Some(early) => early,
            None => item
                .with_subject(|s| engine.decide(s, &info, &snapshot))
                .unwrap_or_else(|| {
                    rejected(
                        RejectionReason::Error,
                        "Unexpected error processing release",
                    )
                }),
        };
        items.push(item);
    }
    Ok(TvEvaluation { items, snapshot })
}
