//! The event log: what the program says it is doing, kept where the UI can show it.
//!
//! A `tracing` layer turns events into rows. The database is opened after
//! logging starts, so events queue in a channel until a writer is attached.

use std::fmt::Write as _;

use chrono::{Duration, Utc};
use omp_db::{Db, repo::LogEntry};
use tokio::sync::mpsc;
use tracing::{
    Event, Level, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{Layer, layer::Context};

pub struct DbLogLayer {
    tx: mpsc::UnboundedSender<LogEntry>,
}

pub struct LogReceiver {
    rx: mpsc::UnboundedReceiver<LogEntry>,
}

pub fn channel() -> (DbLogLayer, LogReceiver) {
    let (tx, rx) = mpsc::unbounded_channel();
    (DbLogLayer { tx }, LogReceiver { rx })
}

#[derive(Default)]
struct Fields {
    message: String,
    extra: String,
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.extra, " {}={value:?}", field.name());
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(self.extra, " {}={value}", field.name());
        }
    }
}

/// Our own code and plugins are logged from INFO up; everything else
/// (HTTP stacks, the torrent engine) only when it is a warning or worse.
fn wanted(target: &str, level: &Level) -> bool {
    let ours = target.starts_with("omp") || target == "plugin";
    if ours {
        *level <= Level::INFO
    } else {
        *level <= Level::WARN
    }
}

impl<S: Subscriber> Layer<S> for DbLogLayer {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let meta = event.metadata();
        if !wanted(meta.target(), meta.level()) {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let level = match *meta.level() {
            Level::ERROR => "error",
            Level::WARN => "warn",
            Level::INFO => "info",
            _ => "debug",
        };
        let _ = self.tx.send(LogEntry {
            time: Utc::now(),
            level: level.into(),
            logger: meta.target().to_owned(),
            message: format!("{}{}", fields.message, fields.extra),
        });
    }
}

const KEEP_DAYS: i64 = 14;
const KEEP_ROWS: i64 = 50_000;

impl LogReceiver {
    /// Writes queued events to the log database until the process ends.
    pub fn spawn_writer(mut self, db: Db) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut purge = tokio::time::interval(std::time::Duration::from_secs(3600));
            loop {
                tokio::select! {
                    first = self.rx.recv() => {
                        let Some(first) = first else { return };
                        let mut batch = vec![first];
                        while batch.len() < 200 {
                            match self.rx.try_recv() {
                                Ok(e) => batch.push(e),
                                Err(_) => break,
                            }
                        }
                        // Logging must never take the program down; a failed write is dropped.
                        let _ = db.logs().insert_batch(&batch).await;
                    }
                    _ = purge.tick() => {
                        let _ = db.logs().purge(Utc::now() - Duration::days(KEEP_DAYS), KEEP_ROWS).await;
                    }
                }
            }
        })
    }
}
