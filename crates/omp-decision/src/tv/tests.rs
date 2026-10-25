use chrono::{DateTime, Duration, Utc};
use omp_core::{
    quality::Quality,
    release::{DownloadProtocol, IndexerFlags, ReleaseInfo},
    tv::SeriesType,
};
use omp_parser::tv::{ParsedEpisodeInfo, parse_title};
use omp_quality::{
    definition::QualityDefinitions, delay::DelayProfile, profile::QualityProfile,
    release_profile::ReleaseProfile,
};

use super::*;
use crate::{
    model::{DecisionConfig, HistoryEntry, IndexerInfo, ReleaseSource},
    reason::RejectionReason as R,
    testkit::{self, model, now},
};

struct Env {
    inner: testkit::Env,
    queue: Vec<TvQueueEntry>,
    pending: bool,
}

impl TvDecisionData for Env {
    fn now(&self) -> DateTime<Utc> {
        now()
    }
    fn config(&self) -> &DecisionConfig {
        &self.inner.config
    }
    fn quality_definitions(&self) -> &QualityDefinitions {
        &self.inner.definitions
    }
    fn delay_profile_for(&self, tags: &[i32]) -> &DelayProfile {
        omp_quality::delay::best_for_tags(&self.inner.delay_profiles, tags).unwrap()
    }
    fn is_blocklisted(&self, _: i64, _: &ReleaseInfo) -> bool {
        self.inner.blocklisted
    }
    fn indexer_blocked_till(&self, _: i32) -> Option<DateTime<Utc>> {
        None
    }
    fn indexer(&self, id: i32) -> Option<IndexerInfo> {
        self.inner.indexers.iter().find(|i| i.id == id).cloned()
    }
    fn release_profiles(&self, tags: &[i32], indexer_id: i32) -> Vec<ReleaseProfile> {
        ReleaseProfile::enabled_for(&self.inner.release_profiles, tags, indexer_id)
            .into_iter()
            .cloned()
            .collect()
    }
    fn history_for_episode(&self, _: i64) -> Vec<HistoryEntry> {
        vec![]
    }
    fn queue_for_series(&self, _: i64) -> Vec<TvQueueEntry> {
        self.queue.clone()
    }
    fn has_pending_release(&self, _: i64, _: &[i64]) -> bool {
        self.pending
    }
    fn oldest_pending_age_minutes(&self, _: i64, _: &[i64]) -> Option<f64> {
        None
    }
    fn free_space(&self, _: &str) -> Option<u64> {
        None
    }
}

fn env() -> Env {
    Env {
        inner: testkit::Env::new(),
        queue: vec![],
        pending: false,
    }
}

struct Fx {
    release: ReleaseInfo,
    parsed: ParsedEpisodeInfo,
    series: SeriesView,
    episodes: Vec<EpisodeView>,
    files: Vec<EpisodeFileView>,
    profile: QualityProfile,
}

fn episode(id: i64, season: i32, number: i32, aired_days_ago: i64) -> EpisodeView {
    EpisodeView {
        id,
        season_number: season,
        episode_number: number,
        runtime: 45,
        air_date_utc: Some(now() - Duration::days(aired_days_ago)),
        monitored: true,
        file_id: None,
    }
}

fn fx(title: &str) -> Fx {
    let parsed = parse_title(title).expect("parses");
    let numbers = parsed.episode_numbers.clone();
    Fx {
        release: ReleaseInfo {
            guid: "g".into(),
            title: title.into(),
            size: 1_500_000_000,
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
        },
        episodes: numbers
            .iter()
            .map(|n| episode(i64::from(*n), parsed.season_number().unwrap_or(1), *n, 10))
            .collect(),
        parsed,
        series: SeriesView {
            id: 1,
            title: "Some Show".into(),
            series_type: SeriesType::Standard,
            monitored: true,
            tags: vec![],
            runtime: 45,
            path: "/tv/Some Show".into(),
            first_season: Some(1),
            first_season_start: None,
        },
        files: vec![],
        profile: testkit::profile(),
    }
}

impl Fx {
    fn subject(&self) -> EpisodeSubject<'_> {
        EpisodeSubject {
            release: &self.release,
            parsed: &self.parsed,
            series: &self.series,
            episodes: &self.episodes,
            files: &self.files,
            profile: &self.profile,
            custom_format_ids: &[],
            custom_format_score: 0,
            source: ReleaseSource::Rss,
            mapped_season: self.parsed.season_number(),
        }
    }

    fn reasons(&self, info: &TvDecisionInfo, env: &Env) -> Vec<R> {
        TvDecisionEngine::default()
            .decide(&self.subject(), info, env)
            .rejections
            .iter()
            .map(|r| r.reason)
            .collect()
    }
}

fn search(kind: TvSearchKind, episode_ids: Vec<i64>) -> TvDecisionInfo {
    TvDecisionInfo {
        pushed_release: false,
        search: Some(TvSearchContext {
            series_id: 1,
            episode_ids,
            episodes: vec![],
            kind,
            user_invoked: false,
            interactive: false,
            monitored_only: true,
        }),
    }
}

const SHOW: &str = "Some.Show.S01E02.1080p.WEB-DL.x264-GRP";

#[test]
fn a_wanted_episode_is_accepted() {
    assert_eq!(
        fx(SHOW).reasons(&TvDecisionInfo::default(), &env()),
        Vec::<R>::new()
    );
}

#[test]
fn unmonitored_series_and_episodes_are_refused() {
    let mut f = fx(SHOW);
    f.series.monitored = false;
    assert_eq!(
        f.reasons(&TvDecisionInfo::default(), &env()),
        [R::SeriesNotMonitored]
    );
    let mut f = fx(SHOW);
    f.episodes[0].monitored = false;
    assert_eq!(
        f.reasons(&TvDecisionInfo::default(), &env()),
        [R::EpisodeNotMonitored]
    );
    // An interactive pick may ignore monitoring.
    let mut info = search(
        TvSearchKind::SingleEpisode {
            season: 1,
            episode: 2,
        },
        vec![2],
    );
    info.search.as_mut().unwrap().monitored_only = false;
    assert!(f.reasons(&info, &env()).is_empty());
}

#[test]
fn size_is_judged_against_the_total_runtime() {
    let mut f = fx("Some.Show.S01E02E03.1080p.WEB-DL.x264-GRP");
    // 90 minutes at the default 100 MB/min cap is 9 GB; 4 GB passes, 12 GB does not.
    f.release.size = 4_000_000_000;
    assert!(f.reasons(&TvDecisionInfo::default(), &env()).is_empty());
    f.release.size = 12_000_000_000;
    assert_eq!(
        f.reasons(&TvDecisionInfo::default(), &env()),
        [R::AboveMaximumSize]
    );
    // No runtime anywhere means the size cannot be judged.
    let mut f = fx(SHOW);
    f.series.runtime = 0;
    f.episodes[0].runtime = 0;
    f.series.first_season = None;
    assert_eq!(
        f.reasons(&TvDecisionInfo::default(), &env()),
        [R::UnknownRuntime]
    );
}

#[test]
fn season_packs_wait_for_every_episode_to_air() {
    let mut f = fx("Some.Show.S01.1080p.WEB-DL.x264-GRP");
    f.episodes = vec![episode(1, 1, 1, 10), episode(2, 1, 2, 10)];
    assert!(f.reasons(&TvDecisionInfo::default(), &env()).is_empty());
    f.episodes[1].air_date_utc = Some(now() + Duration::days(3));
    assert_eq!(
        f.reasons(&TvDecisionInfo::default(), &env()),
        [R::FullSeasonNotAired]
    );
}

#[test]
fn multi_season_and_split_episodes_are_not_supported() {
    let mut f = fx("Some.Show.S01-S03.1080p.BluRay.x264-GRP");
    f.episodes = vec![episode(1, 1, 1, 10)];
    assert!(
        f.reasons(&TvDecisionInfo::default(), &env())
            .contains(&R::MultiSeason)
    );
    let mut f = fx(SHOW);
    f.parsed.is_split_episode = true;
    assert_eq!(
        f.reasons(&TvDecisionInfo::default(), &env()),
        [R::SplitEpisode]
    );
}

#[test]
fn searches_only_accept_what_was_asked_for() {
    let f = fx(SHOW);
    let wrong_ep = search(
        TvSearchKind::SingleEpisode {
            season: 1,
            episode: 5,
        },
        vec![5],
    );
    let r = f.reasons(&wrong_ep, &env());
    assert!(r.contains(&R::WrongEpisode));
    let wrong_season = search(
        TvSearchKind::SingleEpisode {
            season: 2,
            episode: 2,
        },
        vec![2],
    );
    assert!(f.reasons(&wrong_season, &env()).contains(&R::WrongSeason));
    let right = search(
        TvSearchKind::SingleEpisode {
            season: 1,
            episode: 2,
        },
        vec![2],
    );
    assert!(f.reasons(&right, &env()).is_empty());
    let mut other = right.clone();
    other.search.as_mut().unwrap().series_id = 9;
    assert!(f.reasons(&other, &env()).contains(&R::WrongSeries));
    // A season pack is not a single episode.
    let pack = fx("Some.Show.S01.1080p.WEB-DL.x264-GRP");
    assert!(pack.reasons(&right, &env()).contains(&R::FullSeason));
}

#[test]
fn existing_files_decide_upgrades() {
    let mut f = fx(SHOW);
    f.episodes[0].file_id = Some(10);
    f.files = vec![EpisodeFileView {
        id: 10,
        quality: model(Quality::WEBDL_1080P),
        release_group: Some("GRP".into()),
        date_added: now() - Duration::days(30),
        custom_format_ids: vec![],
        episode_ids: vec![2],
    }];
    // Same quality as the file, which already meets the cutoff.
    assert!(
        f.reasons(&TvDecisionInfo::default(), &env())
            .contains(&R::DiskCutoffMet)
    );
    // The file is lower than the cutoff and this is better: an upgrade.
    f.files[0].quality = model(Quality::WEBDL_720P);
    assert!(f.reasons(&TvDecisionInfo::default(), &env()).is_empty());
    // A file that holds more episodes than the release is never replaced.
    f.files[0].episode_ids = vec![2, 3];
    assert!(
        f.reasons(&TvDecisionInfo::default(), &env())
            .contains(&R::ExistingFileHasMoreEpisodes)
    );
}

#[test]
fn the_queue_and_pending_releases_block_duplicates() {
    let f = fx(SHOW);
    let mut e = env();
    e.queue = vec![TvQueueEntry {
        episode_ids: vec![2],
        quality: model(Quality::WEBDL_1080P),
        custom_format_ids: vec![],
        failed_pending: false,
    }];
    assert!(
        f.reasons(&TvDecisionInfo::default(), &e)
            .contains(&R::QueueCutoffMet)
    );
    e.queue[0].episode_ids = vec![99];
    assert!(f.reasons(&TvDecisionInfo::default(), &e).is_empty());

    let mut e = env();
    e.pending = true;
    let mut f = fx(SHOW);
    let s = |f: &Fx, e: &Env| {
        let mut subject = f.subject();
        subject.source = ReleaseSource::Search;
        TvDecisionEngine::default()
            .decide(&subject, &TvDecisionInfo::default(), e)
            .rejections
    };
    let rej = s(&f, &e);
    assert_eq!(rej.len(), 1);
    assert_eq!(rej[0].reason, R::MinimumAgeDelayPushed);
    f.release.title = "x".into();
}

#[test]
fn a_delay_profile_holds_releases_back() {
    let mut e = env();
    e.inner.delay_profiles = vec![DelayProfile {
        usenet_delay: 120,
        ..DelayProfile::default_profile()
    }];
    let f = fx(SHOW);
    let d = TvDecisionEngine::default().decide(&f.subject(), &TvDecisionInfo::default(), &e);
    assert!(d.temporarily_rejected(), "{d:?}");
}

#[test]
fn season_packs_rank_above_single_episodes_and_better_quality_above_both() {
    let pack = fx("Some.Show.S01.1080p.WEB-DL.x264-GRP");
    let mut pack = pack;
    pack.episodes = vec![episode(1, 1, 1, 10), episode(2, 1, 2, 10)];
    let single = fx(SHOW);
    let mut better = fx("Some.Show.S01E02.1080p.BluRay.x264-GRP");
    better.episodes = single.episodes.clone();
    let e = env();
    let (p, s, b) = (pack.subject(), single.subject(), better.subject());
    assert_eq!(compare::compare(&p, &s, &e), std::cmp::Ordering::Greater);
    assert_eq!(
        compare::compare(&b, &p, &e),
        std::cmp::Ordering::Greater,
        "quality outranks pack-ness"
    );
    let mut all = vec![&s, &p, &b];
    compare::sort_best_first(&mut all, &e);
    assert!(std::ptr::eq(all[0], &b) && std::ptr::eq(all[2], &s));
}

#[test]
fn air_date_restriction_holds_back_releases_published_before_the_episode_aired() {
    let mut f = fx(SHOW);
    // The release came out an hour ago; the episode airs in two hours.
    f.episodes[0].air_date_utc = Some(now() + Duration::hours(2));
    let mut e = env();
    e.inner.release_profiles = vec![ReleaseProfile {
        id: 1,
        name: "air".into(),
        enabled: true,
        required: vec![],
        ignored: vec![],
        indexer_id: 0,
        tags: vec![],
        air_date_restriction: true,
        air_date_grace_period_hours: 0,
    }];
    assert_eq!(
        f.reasons(&TvDecisionInfo::default(), &e),
        [R::BeforeAirDate]
    );
    // Once it has aired the release is fine, and without the setting it always is.
    f.episodes[0].air_date_utc = Some(now() - Duration::hours(5));
    assert!(f.reasons(&TvDecisionInfo::default(), &e).is_empty());
    e.inner.release_profiles[0].air_date_restriction = false;
    f.episodes[0].air_date_utc = Some(now() + Duration::hours(2));
    assert!(f.reasons(&TvDecisionInfo::default(), &e).is_empty());
}

#[test]
fn old_seasons_ask_for_the_pack_when_the_indexer_says_so() {
    let f = fx(SHOW);
    let mut e = env();
    e.inner.indexers = vec![IndexerInfo {
        id: 1,
        season_pack_max_single_episode_age_days: 7,
        ..Default::default()
    }];
    let mut info = search(TvSearchKind::Season { season: 1 }, vec![1, 2]);
    // A season whose last episode aired a month ago.
    info.search.as_mut().unwrap().episodes = vec![
        (1, Some(now() - Duration::days(40))),
        (1, Some(now() - Duration::days(30))),
    ];
    assert!(f.reasons(&info, &e).contains(&R::NotSeasonPack));
    // A recent season still takes single episodes.
    info.search.as_mut().unwrap().episodes = vec![
        (1, Some(now() - Duration::days(3))),
        (1, Some(now() - Duration::days(10))),
    ];
    assert!(!f.reasons(&info, &e).contains(&R::NotSeasonPack));
    // A single-episode search never insists on a pack.
    info.search.as_mut().unwrap().episodes = vec![(1, Some(now() - Duration::days(40)))];
    assert!(!f.reasons(&info, &e).contains(&R::NotSeasonPack));
}
