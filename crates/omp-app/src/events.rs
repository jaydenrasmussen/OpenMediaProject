//! Live updates over a WebSocket: the server tells the UI that something
//! changed and the UI reloads what it shows. Events carry no data, only a
//! kind, so a missed one costs nothing but a stale screen until the next.
//!
//! Two sources feed it: every successful change through the API, and a
//! watcher that notices changes made by background jobs (the queue, history
//! and scheduled tasks).

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::Arc,
    time::Duration,
};

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use tokio::sync::broadcast;

use crate::app::App;

/// What changed. The UI maps these to the screens that care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A download appeared, moved on or finished.
    Queue,
    /// New history rows (grabs, imports, failures).
    History,
    /// A scheduled task ran.
    Tasks,
    /// Someone changed something through the API.
    Changed,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Queue => "queue",
            Kind::History => "history",
            Kind::Tasks => "tasks",
            Kind::Changed => "changed",
        }
    }
}

pub struct Events {
    tx: broadcast::Sender<Kind>,
}

impl Default for Events {
    fn default() -> Self {
        Self::new()
    }
}

impl Events {
    pub fn new() -> Self {
        Self {
            tx: broadcast::channel(64).0,
        }
    }

    /// Tells every connected client. Nobody listening is not an error.
    pub fn send(&self, kind: Kind) {
        let _ = self.tx.send(kind);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Kind> {
        self.tx.subscribe()
    }

    pub fn listeners(&self) -> usize {
        self.tx.receiver_count()
    }
}

fn hash_of(value: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

/// What the watcher compares from one look to the next.
#[derive(Default, PartialEq, Eq)]
struct Fingerprint {
    queue: u64,
    history: (i64, i64),
    tasks: u64,
}

async fn fingerprint(app: &App) -> Fingerprint {
    let queue = hash_of(
        app.monitor
            .snapshot()
            .iter()
            .map(|t| {
                (
                    t.item.download_id.clone(),
                    format!("{:?}", t.state),
                    format!("{:?}", t.item.status),
                    t.item.remaining_size,
                )
            })
            .collect::<Vec<_>>(),
    );
    let movie = app.db.history().last_id().await.unwrap_or(0);
    let episode = app.db.episode_history().last_id().await.unwrap_or(0);
    let tasks = hash_of(
        app.task_statuses()
            .iter()
            .map(|t| (t.name, t.last_run))
            .collect::<Vec<_>>(),
    );
    Fingerprint {
        queue,
        history: (movie, episode),
        tasks,
    }
}

/// Starts the watcher. It only looks while somebody is connected.
pub fn spawn_watcher(app: Arc<App>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last: Option<Fingerprint> = None;
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if app.events.listeners() == 0 {
                last = None;
                continue;
            }
            let now = fingerprint(&app).await;
            if let Some(before) = &last {
                if before.queue != now.queue {
                    app.events.send(Kind::Queue);
                }
                if before.history != now.history {
                    app.events.send(Kind::History);
                }
                if before.tasks != now.tasks {
                    app.events.send(Kind::Tasks);
                }
            }
            last = Some(now);
        }
    })
}

/// Middleware: a change made through the API tells listeners to reload.
pub async fn announce_changes(
    State(app): State<Arc<App>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let mutating = !matches!(
        req.method(),
        &axum::http::Method::GET | &axum::http::Method::HEAD | &axum::http::Method::OPTIONS
    );
    let response = next.run(req).await;
    if mutating && response.status().is_success() {
        app.events.send(Kind::Changed);
    }
    response
}

/// A browser sends `Origin` on WebSocket handshakes and cookies ride along,
/// so a page on another site could otherwise listen in. Only same-origin
/// handshakes pass; clients that send no `Origin` (scripts) are fine, as
/// they had to authenticate with the API key anyway.
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok());
    let origin_host = origin
        .to_str()
        .ok()
        .and_then(|o| o.split_once("://"))
        .map(|(_, rest)| rest.trim_end_matches('/'));
    matches!((host, origin_host), (Some(h), Some(o)) if h == o)
}

pub async fn websocket(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !same_origin(&headers) {
        return (StatusCode::FORBIDDEN, "cross-origin WebSocket refused").into_response();
    }
    upgrade.on_upgrade(move |socket| serve(app, socket))
}

async fn serve(app: Arc<App>, mut socket: WebSocket) {
    let mut events = app.events.subscribe();
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    ping.tick().await; // the first tick is immediate
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(kind) => {
                    let text = format!(r#"{{"kind":"{}"}}"#, kind.name());
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        return;
                    }
                }
                // Missed some: say so, the client reloads everything.
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    if socket.send(Message::Text(r#"{"kind":"changed"}"#.into())).await.is_err() {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                Some(Ok(_)) => {}
            },
            _ = ping.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoProbe;

    #[async_trait::async_trait]
    impl omp_services::MediaProbe for NoProbe {
        async fn probe(&self, _: &std::path::Path) -> Option<omp_services::ProbeInfo> {
            None
        }
    }

    #[tokio::test]
    async fn the_watcher_announces_what_background_jobs_change() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(
            crate::AppConfig::new(dir.path().join("data")),
            omp_db::Db::in_memory().await.unwrap(),
            None,
            Arc::new(NoProbe),
            Arc::new(omp_services::NoDiskCheck),
        )
        .await
        .unwrap();
        app.register_task("t", "Test job", Duration::from_secs(60));
        let mut events = app.events.subscribe();
        let _watcher = spawn_watcher(app.clone());
        // The watcher only looks while someone listens; give it a first look.
        tokio::time::sleep(Duration::from_millis(2500)).await;
        app.finish_task("t", chrono::Utc::now(), None);
        let kind = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("an event")
            .unwrap();
        assert_eq!(kind, Kind::Tasks);
    }

    #[test]
    fn only_same_origin_handshakes_pass() {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, "omp.local:8080".parse().unwrap());
        assert!(same_origin(&h), "no origin: a script");
        h.insert(header::ORIGIN, "http://omp.local:8080".parse().unwrap());
        assert!(same_origin(&h));
        h.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
        assert!(!same_origin(&h));
        h.insert(header::ORIGIN, "null".parse().unwrap());
        assert!(!same_origin(&h));
    }
}
