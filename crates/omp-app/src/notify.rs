//! Sends events to notification plugins.
//!
//! Events are read from the history table rather than hooked into each code
//! path, so a grab made from the API, the scheduler or a search all notify
//! the same way. A cursor in the config table remembers how far we got, so a
//! restart neither repeats nor (much) skips events.

use omp_core::history::HistoryEvent;
use omp_db::repo::{Provider, ProviderKind};
use omp_plugin_host::{Event, MovieInfo, SeriesInfo};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::App;

const CURSOR_KEY: &str = "notification_cursor";
const EPISODE_CURSOR_KEY: &str = "notification_cursor_episodes";

/// Event kinds a notification can subscribe to.
pub const TRIGGERS: &[&str] = &[
    "grab",
    "import",
    "download-failed",
    "rename",
    "file-delete",
    "health-issue",
    "health-restored",
    "upgrade",
    "movie-add",
    "movie-delete",
    "series-add",
    "series-delete",
    "manual-interaction",
    "app-update",
];

/// Kinds that arrive in bulk (a rename of a whole library); one announcement
/// covers a batch.
fn is_batch(kind: &str) -> bool {
    matches!(kind, "rename" | "file-delete")
}

fn wants(p: &Provider, kind: &str) -> bool {
    NotificationSettings::from_provider(p)
        .triggers
        .iter()
        .any(|t| t == kind)
}

/// Does anyone want an event of this kind? An upgrade is also an import, so
/// those who only asked for imports still hear about it.
fn has_subscribers(targets: &[Provider], kind: &str) -> bool {
    targets
        .iter()
        .any(|p| wants(p, kind) || (kind == "upgrade" && wants(p, "import")))
}

/// Sends an event to everyone who wants it. An upgrade goes out as an
/// upgrade to those who asked for upgrades and as a plain import to those
/// who only asked for imports.
async fn broadcast(app: &App, targets: &[Provider], event: &Event) -> usize {
    let as_import = Event {
        kind: "import".into(),
        title: event.title.replace(" upgraded", " imported"),
        ..event.clone()
    };
    let mut sent = 0;
    for p in targets {
        let event = if wants(p, &event.kind) {
            event
        } else if event.kind == "upgrade" && wants(p, "import") {
            &as_import
        } else {
            continue;
        };
        match send(app, p, event).await {
            Ok(()) => sent += 1,
            Err(e) => tracing::warn!(notification = %p.name, "notification failed: {e}"),
        }
    }
    sent
}

async fn enabled_targets(app: &App) -> anyhow::Result<Vec<Provider>> {
    Ok(app
        .db
        .providers()
        .list(ProviderKind::Notification)
        .await?
        .into_iter()
        .filter(|p| p.enabled)
        .collect())
}

pub fn movie_info(m: &omp_db::repo::MovieRecord) -> MovieInfo {
    MovieInfo {
        title: m.movie.title.clone(),
        year: m.movie.year.max(0) as u32,
        tmdb_id: m.movie.tmdb_id,
        imdb_id: m.metadata.imdb_id.clone(),
    }
}

/// Queues an event that history doesn't record (a movie added, a series
/// deleted); the next dispatch sends it.
pub(crate) fn announce(app: &App, event: Event) {
    let mut outbox = app.outbox.lock().unwrap_or_else(|e| e.into_inner());
    if outbox.len() < 1000 {
        outbox.push(event);
    }
}

pub(crate) fn movie_event(kind: &str, title: &str, m: &omp_db::repo::MovieRecord) -> Event {
    event_for(
        kind,
        title,
        format!("{} ({})", m.movie.title, m.movie.year),
        Some(movie_info(m)),
    )
}

pub(crate) fn series_event(kind: &str, title: &str, s: &omp_core::tv::SeriesInfo) -> Event {
    Event {
        series: Some(series_info(s)),
        ..event_for(kind, title, format!("{} ({})", s.title, s.year), None)
    }
}

/// Queues "series added" for a series just stored, whichever path added it.
pub(crate) async fn announce_series_added(app: &App, id: i64) {
    if let Ok(s) = app.db.series().get(id).await {
        announce(app, series_event("series-add", "Series added", &s.info));
    }
}

/// As [`announce_series_added`] for a movie.
pub(crate) async fn announce_movie_added(app: &App, id: i64) {
    if let Ok(m) = app.db.movies().get(id).await {
        announce(app, movie_event("movie-add", "Movie added", &m));
    }
}

async fn dispatch_outbox(app: &App) -> anyhow::Result<usize> {
    let events = std::mem::take(&mut *app.outbox.lock().unwrap_or_else(|e| e.into_inner()));
    if events.is_empty() {
        return Ok(0);
    }
    let targets = enabled_targets(app).await?;
    let mut sent = 0;
    for event in &events {
        sent += broadcast(app, &targets, event).await;
    }
    Ok(sent)
}

/// The trigger and title for a history event, or `None` when it isn't announced.
fn classify(event: HistoryEvent, noun: &str, upgrade: bool) -> Option<(&'static str, String)> {
    Some(match event {
        HistoryEvent::DownloadFolderImported if upgrade => ("upgrade", format!("{noun} upgraded")),
        HistoryEvent::Grabbed => ("grab", "Release grabbed".into()),
        HistoryEvent::DownloadFolderImported => ("import", format!("{noun} imported")),
        HistoryEvent::DownloadFailed => ("download-failed", "Download failed".into()),
        HistoryEvent::MovieFileRenamed => ("rename", format!("{noun} renamed")),
        HistoryEvent::MovieFileDeleted => ("file-delete", format!("{noun} file deleted")),
        _ => return None,
    })
}

/// How a notification provider's `settings` is laid out: which events it
/// wants, and the plugin's own settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationSettings {
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub settings: Value,
}

impl NotificationSettings {
    pub fn from_provider(p: &Provider) -> Self {
        serde_json::from_value(p.settings.clone()).unwrap_or(Self {
            triggers: Vec::new(),
            settings: Value::Null,
        })
    }
}

/// Sends one event to one notification provider.
pub async fn send(app: &App, p: &Provider, event: &Event) -> Result<(), String> {
    if p.implementation == crate::email::IMPLEMENTATION {
        return crate::email::send(&NotificationSettings::from_provider(p).settings, event).await;
    }
    let plugin_id = p.plugin_id.as_deref().ok_or("no plugin chosen")?;
    let plugin = app
        .plugins()
        .notification(plugin_id)
        .ok_or_else(|| format!("plugin '{plugin_id}' isn't installed"))?;
    let settings = NotificationSettings::from_provider(p).settings;
    plugin
        .send(event, &settings.to_string())
        .await
        .map_err(|e| format!("{e:#}"))
}

async fn movie_for(app: &App, id: i64) -> Option<MovieInfo> {
    app.db.movies().get(id).await.ok().map(|m| movie_info(&m))
}

fn event_for(kind: &str, title: &str, message: String, movie: Option<MovieInfo>) -> Event {
    Event {
        kind: kind.into(),
        title: title.into(),
        message,
        movie,
        series: None,
    }
}

pub fn series_info(s: &omp_core::tv::SeriesInfo) -> SeriesInfo {
    SeriesInfo {
        title: s.title.clone(),
        year: s.year.max(0) as u32,
        tmdb_id: s.tmdb_id,
        tvdb_id: s.tvdb_id,
    }
}

pub fn test_event() -> Event {
    event_for(
        "test",
        "Test notification",
        "If you can read this, the notification works.".into(),
        None,
    )
}

/// Delivers history events recorded since the last call. Returns how many
/// notifications were sent.
pub async fn dispatch_once(app: &App) -> anyhow::Result<usize> {
    Ok(dispatch_movies(app).await? + dispatch_episodes(app).await? + dispatch_outbox(app).await?)
}

async fn dispatch_movies(app: &App) -> anyhow::Result<usize> {
    let cursor = match app.db.config().get(CURSOR_KEY).await? {
        Some(v) => v.parse::<i64>().unwrap_or(0),
        None => {
            // First run: start from now rather than announcing the whole past.
            let last = app.db.history().last_id().await?;
            app.db.config().set(CURSOR_KEY, &last.to_string()).await?;
            return Ok(0);
        }
    };
    let events = app.db.history().after(cursor, 100).await?;
    let Some(newest) = events.last().map(|e| e.id) else {
        return Ok(0);
    };
    let targets = enabled_targets(app).await?;

    let mut sent = 0;
    let mut batches: std::collections::BTreeMap<
        &'static str,
        (String, Vec<&omp_db::repo::HistoryRecord>),
    > = Default::default();
    for h in &events {
        let Some((kind, title)) = classify(h.event, "Movie", h.data.contains_key("upgrade")) else {
            continue;
        };
        if is_batch(kind) {
            batches.entry(kind).or_insert((title, Vec::new())).1.push(h);
            continue;
        }
        if !has_subscribers(&targets, kind) {
            continue;
        }
        let movie = movie_for(app, h.movie_id).await;
        let mut message = format!("{} ({})", h.source_title, h.quality.quality.name);
        if let Some(reason) = h.data.get("message").filter(|m| !m.is_empty()) {
            message = format!("{message}: {reason}");
        }
        sent += broadcast(app, &targets, &event_for(kind, &title, message, movie)).await;
    }
    for (kind, (title, rows)) in batches {
        if !has_subscribers(&targets, kind) {
            continue;
        }
        let event = match rows.as_slice() {
            [one] => event_for(
                kind,
                &title,
                one.source_title.clone(),
                movie_for(app, one.movie_id).await,
            ),
            _ => {
                let titles: Vec<&str> = rows.iter().map(|r| r.source_title.as_str()).collect();
                event_for(
                    kind,
                    &format!("{title} ({} files)", rows.len()),
                    batch_message(&titles),
                    None,
                )
            }
        };
        sent += broadcast(app, &targets, &event).await;
    }
    app.db.config().set(CURSOR_KEY, &newest.to_string()).await?;
    Ok(sent)
}

/// The first few titles of a batch, then how many more there were.
fn batch_message(titles: &[&str]) -> String {
    const SHOWN: usize = 5;
    let mut message = titles[..titles.len().min(SHOWN)].join("\n");
    if titles.len() > SHOWN {
        message.push_str(&format!("\n…and {} more", titles.len() - SHOWN));
    }
    message
}

fn episode_label(season: i32, episodes: &[i32]) -> String {
    match episodes {
        [] => format!("Season {season}"),
        [one] => format!("S{season:02}E{one:02}"),
        [first, .., last] => format!("S{season:02}E{first:02}-E{last:02}"),
    }
}

/// As [`dispatch_movies`] for episode history. A release that covers
/// several episodes (a season pack) is announced once.
async fn dispatch_episodes(app: &App) -> anyhow::Result<usize> {
    let cursor = match app.db.config().get(EPISODE_CURSOR_KEY).await? {
        Some(v) => v.parse::<i64>().unwrap_or(0),
        None => {
            let last = app.db.episode_history().last_id().await?;
            app.db
                .config()
                .set(EPISODE_CURSOR_KEY, &last.to_string())
                .await?;
            return Ok(0);
        }
    };
    let events = app.db.episode_history().after(cursor, 200).await?;
    let Some(newest) = events.last().map(|e| e.id) else {
        return Ok(0);
    };
    let targets = enabled_targets(app).await?;

    // One announcement per (event, download), per series for batch kinds, or
    // per row when there is no download.
    let mut groups: Vec<(
        &'static str,
        String,
        Vec<&omp_db::repo::EpisodeHistoryRecord>,
    )> = Vec::new();
    for h in &events {
        let Some((kind, title)) = classify(h.event, "Episode", h.data.contains_key("upgrade"))
        else {
            continue;
        };
        let existing = if is_batch(kind) {
            groups
                .iter_mut()
                .find(|(k, _, rows)| *k == kind && rows[0].series_id == h.series_id)
        } else {
            h.download_id.as_ref().and_then(|d| {
                groups
                    .iter_mut()
                    .find(|(k, _, rows)| *k == kind && rows[0].download_id.as_ref() == Some(d))
            })
        };
        match existing {
            Some((_, _, rows)) => rows.push(h),
            None => groups.push((kind, title, vec![h])),
        }
    }

    let mut sent = 0;
    for (kind, title, rows) in groups {
        if !has_subscribers(&targets, kind) {
            continue;
        }
        let first = rows[0];
        let series = app
            .db
            .series()
            .get(first.series_id)
            .await
            .ok()
            .map(|s| s.info);
        let series_title = series
            .as_ref()
            .map(|s| s.title.as_str())
            .unwrap_or_default();
        let mut by_season: std::collections::BTreeMap<i32, Vec<i32>> = Default::default();
        for h in &rows {
            if let Ok(e) = app.db.episodes().get(h.episode_id).await {
                by_season
                    .entry(e.season_number)
                    .or_default()
                    .push(e.episode_number);
            }
        }
        let labels: Vec<String> = by_season
            .iter_mut()
            .map(|(season, eps)| {
                eps.sort_unstable();
                episode_label(*season, eps)
            })
            .collect();
        let mut message = if is_batch(kind) {
            format!("{series_title} {}", labels.join(", "))
        } else {
            format!(
                "{series_title} {}: {} ({})",
                labels.join(", "),
                first.source_title,
                first.quality.quality.name
            )
        };
        if let Some(reason) = first.data.get("message").filter(|m| !m.is_empty()) {
            message = format!("{message}: {reason}");
        }
        let event = Event {
            series: series.as_ref().map(series_info),
            ..event_for(kind, &title, message, None)
        };
        sent += broadcast(app, &targets, &event).await;
    }
    app.db
        .config()
        .set(EPISODE_CURSOR_KEY, &newest.to_string())
        .await?;
    Ok(sent)
}

const HEALTH_KEY: &str = "notification_health";

/// What identifies a health issue between checks: its source and message
/// with standalone numbers taken out (paths like `/mnt/disk2` keep theirs), so "3 downloads are stuck" becoming "4" is the
/// same issue and not a new one.
fn health_key(source: &str, message: &str) -> String {
    let words: Vec<&str> = message
        .split(' ')
        .map(|w| {
            let bare = w.trim_end_matches(['.', ',', ':']);
            if !bare.is_empty() && bare.parse::<f64>().is_ok() {
                "#"
            } else {
                w
            }
        })
        .collect();
    format!("{source}: {}", words.join(" "))
}

/// The messages of issues in `now` that `before` lacks.
fn health_new<'a>(
    now: &'a std::collections::BTreeMap<String, String>,
    before: &std::collections::BTreeMap<String, String>,
) -> Vec<&'a String> {
    now.iter()
        .filter(|(k, _)| !before.contains_key(*k))
        .map(|(_, m)| m)
        .collect()
}

/// Announces health issues that appeared or cleared since the last call.
/// Returns how many notifications were sent.
pub async fn dispatch_health(app: &App) -> anyhow::Result<usize> {
    let targets = enabled_targets(app).await?;
    if !has_subscribers(&targets, "health-issue") && !has_subscribers(&targets, "health-restored") {
        // Forget the baseline: it would be stale by the time someone subscribes.
        if app.db.config().get(HEALTH_KEY).await?.is_some() {
            app.db.config().set(HEALTH_KEY, "").await?;
        }
        return Ok(0);
    }

    let now: std::collections::BTreeMap<String, String> = crate::health::check(app)
        .await?
        .into_iter()
        .map(|i| {
            (
                health_key(i.source, &i.message),
                format!("{}: {}", i.source, i.message),
            )
        })
        .collect();
    let previous: Option<std::collections::BTreeMap<String, String>> = app
        .db
        .config()
        .get(HEALTH_KEY)
        .await?
        .and_then(|v| serde_json::from_str(&v).ok());
    let mut sent = 0;
    // Without a baseline, what is already wrong is not news.
    if let Some(previous) = previous {
        for (kind, title, messages) in [
            ("health-issue", "Health issue", health_new(&now, &previous)),
            (
                "health-restored",
                "Health issue resolved",
                health_new(&previous, &now),
            ),
        ] {
            for message in messages {
                let event = event_for(kind, title, message.clone(), None);
                sent += broadcast(app, &targets, &event).await;
            }
        }
    }
    app.db
        .config()
        .set(HEALTH_KEY, &serde_json::to_string(&now)?)
        .await?;
    Ok(sent)
}

const BLOCKED_KEY: &str = "notification_blocked";
const VERSION_KEY: &str = "notification_version";

/// Announces downloads that now need a person, and a new application version.
/// Returns how many notifications were sent.
pub async fn dispatch_system(app: &App) -> anyhow::Result<usize> {
    let targets = enabled_targets(app).await?;
    let mut sent = 0;

    let version = env!("CARGO_PKG_VERSION");
    let previous = app.db.config().get(VERSION_KEY).await?;
    if previous.as_deref() != Some(version) {
        if let Some(old) = previous {
            let event = event_for(
                "app-update",
                "Updated",
                format!("Updated from {old} to {version}."),
                None,
            );
            sent += broadcast(app, &targets, &event).await;
        }
        app.db.config().set(VERSION_KEY, version).await?;
    }

    // The monitor is empty before its first refresh and while one runs; that
    // is not "nothing is blocked".
    if app.monitor.settled() {
        let blocked: Vec<String> = app
            .monitor
            .snapshot()
            .into_iter()
            .filter(|t| t.state == omp_services::TrackedState::ImportBlocked)
            .map(|t| t.item.title)
            .collect();
        let known: Option<Vec<String>> = app
            .db
            .config()
            .get(BLOCKED_KEY)
            .await?
            .and_then(|v| serde_json::from_str(&v).ok());
        if let Some(known) = known {
            for title in blocked.iter().filter(|t| !known.contains(t)) {
                let event = event_for(
                    "manual-interaction",
                    "Manual interaction required",
                    format!("{title} can't be imported automatically."),
                    None,
                );
                sent += broadcast(app, &targets, &event).await;
            }
        }
        app.db
            .config()
            .set(BLOCKED_KEY, &serde_json::to_string(&blocked)?)
            .await?;
    }
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_events_map_to_known_triggers() {
        for event in [
            HistoryEvent::Grabbed,
            HistoryEvent::DownloadFolderImported,
            HistoryEvent::DownloadFailed,
            HistoryEvent::MovieFileRenamed,
            HistoryEvent::MovieFileDeleted,
        ] {
            let (kind, _) = classify(event, "Movie", false).unwrap();
            assert!(TRIGGERS.contains(&kind), "{kind}");
        }
        let (kind, title) = classify(HistoryEvent::DownloadFolderImported, "Movie", true).unwrap();
        assert_eq!((kind, title.as_str()), ("upgrade", "Movie upgraded"));
        assert!(classify(HistoryEvent::DownloadIgnored, "Movie", false).is_none());
        assert_eq!(
            classify(HistoryEvent::MovieFileRenamed, "Episode", false)
                .unwrap()
                .1,
            "Episode renamed"
        );
    }

    #[test]
    fn health_issues_ignore_changing_numbers() {
        assert_eq!(
            health_key("Downloads", "3 finished downloads cannot be imported"),
            health_key("Downloads", "14 finished downloads cannot be imported")
        );
        assert_ne!(
            health_key("Downloads", "3 finished downloads"),
            health_key("Tools", "3 finished downloads")
        );
        assert_eq!(
            health_key("DiskSpace", "Only 0.4 GB is free in /mnt/disk1."),
            health_key("DiskSpace", "Only 12.5 GB is free in /mnt/disk1.")
        );
        assert_ne!(
            health_key("RootFolders", "Root folder /mnt/disk1 is missing."),
            health_key("RootFolders", "Root folder /mnt/disk2 is missing.")
        );
    }

    #[test]
    fn health_new_lists_only_what_is_missing_before() {
        let map = |items: &[(&str, &str)]| -> std::collections::BTreeMap<String, String> {
            items
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let before = map(&[("a", "A"), ("b", "B")]);
        let now = map(&[("b", "B2"), ("c", "C")]);
        assert_eq!(health_new(&now, &before), ["C"]);
        assert_eq!(health_new(&before, &now), ["A"]);
    }
}
