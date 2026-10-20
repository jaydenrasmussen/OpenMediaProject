//! Sends events to notification plugins.
//!
//! Events are read from the history table rather than hooked into each code
//! path, so a grab made from the API, the scheduler or a search all notify
//! the same way. A cursor in the config table remembers how far we got, so a
//! restart neither repeats nor (much) skips events.

use omp_core::history::HistoryEvent;
use omp_db::repo::{Provider, ProviderKind};
use omp_plugin_host::{Event, MovieInfo};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::App;

const CURSOR_KEY: &str = "notification_cursor";

/// Event kinds a notification can subscribe to.
pub const TRIGGERS: &[&str] = &["grab", "import", "download-failed"];

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
    let plugin_id = p.plugin_id.as_deref().ok_or("no plugin chosen")?;
    let plugin = app
        .plugins
        .notification(plugin_id)
        .ok_or_else(|| format!("plugin '{plugin_id}' isn't installed"))?;
    let settings = NotificationSettings::from_provider(p).settings;
    plugin
        .send(event, &settings.to_string())
        .await
        .map_err(|e| format!("{e:#}"))
}

fn event_for(kind: &str, title: &str, message: String, movie: Option<MovieInfo>) -> Event {
    Event {
        kind: kind.into(),
        title: title.into(),
        message,
        movie,
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
    let targets: Vec<Provider> = app
        .db
        .providers()
        .list(ProviderKind::Notification)
        .await?
        .into_iter()
        .filter(|p| p.enabled)
        .collect();

    let mut sent = 0;
    for h in &events {
        let (kind, title) = match h.event {
            HistoryEvent::Grabbed => ("grab", "Release grabbed"),
            HistoryEvent::DownloadFolderImported => ("import", "Movie imported"),
            HistoryEvent::DownloadFailed => ("download-failed", "Download failed"),
            _ => continue,
        };
        let wanting: Vec<&Provider> = targets
            .iter()
            .filter(|p| {
                NotificationSettings::from_provider(p)
                    .triggers
                    .iter()
                    .any(|t| t == kind)
            })
            .collect();
        if wanting.is_empty() {
            continue;
        }
        let movie = app
            .db
            .movies()
            .get(h.movie_id)
            .await
            .ok()
            .map(|m| MovieInfo {
                title: m.movie.title,
                year: m.movie.year.max(0) as u32,
                tmdb_id: m.movie.tmdb_id,
                imdb_id: m.metadata.imdb_id,
            });
        let mut message = format!("{} ({})", h.source_title, h.quality.quality.name);
        if let Some(reason) = h.data.get("message").filter(|m| !m.is_empty()) {
            message = format!("{message}: {reason}");
        }
        let event = event_for(kind, title, message, movie);
        for p in wanting {
            match send(app, p, &event).await {
                Ok(()) => sent += 1,
                Err(e) => tracing::warn!(notification = %p.name, "notification failed: {e}"),
            }
        }
    }
    app.db.config().set(CURSOR_KEY, &newest.to_string()).await?;
    Ok(sent)
}
