//! Providers: indexers, download clients, notifications, import lists and
//! metadata sources share one table and one status/back-off mechanism.

use chrono::{DateTime, Duration, Utc};
use omp_core::backoff::ProviderStatus;
use sqlx::{Row, SqlitePool, sqlite::SqliteRow, types::Json};

use crate::{DbError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    Indexer,
    DownloadClient,
    Notification,
    ImportList,
    Metadata,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::Indexer => "indexer",
            ProviderKind::DownloadClient => "download_client",
            ProviderKind::Notification => "notification",
            ProviderKind::ImportList => "import_list",
            ProviderKind::Metadata => "metadata",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "indexer" => ProviderKind::Indexer,
            "download_client" => ProviderKind::DownloadClient,
            "notification" => ProviderKind::Notification,
            "import_list" => ProviderKind::ImportList,
            "metadata" => ProviderKind::Metadata,
            other => return Err(DbError::Corrupt(format!("unknown provider kind {other}"))),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    pub id: i64,
    pub kind: ProviderKind,
    pub name: String,
    /// "newznab", "torznab", "qbittorrent", ... or a plugin's implementation name.
    pub implementation: String,
    /// `None` for built-in implementations.
    pub plugin_id: Option<String>,
    pub settings: serde_json::Value,
    pub tags: Vec<i32>,
    pub enabled: bool,
    pub enable_rss: bool,
    pub enable_automatic_search: bool,
    pub enable_interactive_search: bool,
    /// 1 (first) to 50.
    pub priority: i32,
}

pub struct Providers<'a>(pub(crate) &'a SqlitePool);

fn provider_from_row(r: &SqliteRow) -> Result<Provider> {
    Ok(Provider {
        id: r.try_get("id")?,
        kind: ProviderKind::parse(&r.try_get::<String, _>("kind")?)?,
        name: r.try_get("name")?,
        implementation: r.try_get("implementation")?,
        plugin_id: r.try_get("plugin_id")?,
        settings: r.try_get::<Json<serde_json::Value>, _>("settings")?.0,
        tags: r.try_get::<Json<Vec<i32>>, _>("tags")?.0,
        enabled: r.try_get("enabled")?,
        enable_rss: r.try_get("enable_rss")?,
        enable_automatic_search: r.try_get("enable_automatic_search")?,
        enable_interactive_search: r.try_get("enable_interactive_search")?,
        priority: r.try_get("priority")?,
    })
}

impl Providers<'_> {
    pub async fn list(&self, kind: ProviderKind) -> Result<Vec<Provider>> {
        sqlx::query("SELECT * FROM providers WHERE kind = ? ORDER BY priority, name")
            .bind(kind.as_str())
            .fetch_all(self.0)
            .await?
            .iter()
            .map(provider_from_row)
            .collect()
    }

    pub async fn get(&self, id: i64) -> Result<Provider> {
        let row = sqlx::query("SELECT * FROM providers WHERE id = ?")
            .bind(id)
            .fetch_optional(self.0)
            .await?
            .ok_or(DbError::NotFound)?;
        provider_from_row(&row)
    }

    pub async fn insert(&self, p: &Provider) -> Result<i64> {
        Ok(sqlx::query(
            "INSERT INTO providers (kind, name, implementation, plugin_id, settings, tags, enabled,
             enable_rss, enable_automatic_search, enable_interactive_search, priority)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(p.kind.as_str())
        .bind(&p.name)
        .bind(&p.implementation)
        .bind(&p.plugin_id)
        .bind(Json(&p.settings))
        .bind(Json(&p.tags))
        .bind(p.enabled)
        .bind(p.enable_rss)
        .bind(p.enable_automatic_search)
        .bind(p.enable_interactive_search)
        .bind(p.priority)
        .execute(self.0)
        .await?
        .last_insert_rowid())
    }

    pub async fn update(&self, p: &Provider) -> Result<()> {
        let done = sqlx::query(
            "UPDATE providers SET name = ?, implementation = ?, plugin_id = ?, settings = ?, tags = ?,
             enabled = ?, enable_rss = ?, enable_automatic_search = ?, enable_interactive_search = ?,
             priority = ? WHERE id = ?",
        )
        .bind(&p.name)
        .bind(&p.implementation)
        .bind(&p.plugin_id)
        .bind(Json(&p.settings))
        .bind(Json(&p.tags))
        .bind(p.enabled)
        .bind(p.enable_rss)
        .bind(p.enable_automatic_search)
        .bind(p.enable_interactive_search)
        .bind(p.priority)
        .bind(p.id)
        .execute(self.0)
        .await?;
        if done.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    /// Deleting a provider deletes its status row (cascade).
    pub async fn delete(&self, id: i64) -> Result<()> {
        let done = sqlx::query("DELETE FROM providers WHERE id = ?")
            .bind(id)
            .execute(self.0)
            .await?;
        if done.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    // ---- status and back-off ------------------------------------------------------------

    pub async fn status(&self, provider_id: i64) -> Result<ProviderStatus> {
        let row = sqlx::query("SELECT * FROM provider_status WHERE provider_id = ?")
            .bind(provider_id)
            .fetch_optional(self.0)
            .await?;
        let Some(r) = row else {
            return Ok(ProviderStatus::default());
        };
        Ok(ProviderStatus {
            initial_failure: r.try_get("initial_failure")?,
            most_recent_failure: r.try_get("most_recent_failure")?,
            escalation_level: r.try_get::<i64, _>("escalation_level")? as u32,
            disabled_till: r.try_get("disabled_till")?,
        })
    }

    async fn save_status(&self, provider_id: i64, s: &ProviderStatus) -> Result<()> {
        sqlx::query(
            "INSERT INTO provider_status (provider_id, initial_failure, most_recent_failure,
             escalation_level, disabled_till) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(provider_id) DO UPDATE SET initial_failure = excluded.initial_failure,
               most_recent_failure = excluded.most_recent_failure,
               escalation_level = excluded.escalation_level, disabled_till = excluded.disabled_till",
        )
        .bind(provider_id)
        .bind(s.initial_failure)
        .bind(s.most_recent_failure)
        .bind(i64::from(s.escalation_level))
        .bind(s.disabled_till)
        .execute(self.0)
        .await?;
        Ok(())
    }

    pub async fn record_success(&self, provider_id: i64) -> Result<()> {
        let mut status = self.status(provider_id).await?;
        if status.record_success() {
            self.save_status(provider_id, &status).await?;
        }
        Ok(())
    }

    /// `escalate` is false for plain connection failures. `minimum_backoff` is
    /// a server-supplied wait (Retry-After). Returns the new status.
    pub async fn record_failure(
        &self,
        provider_id: i64,
        now: DateTime<Utc>,
        process_start: DateTime<Utc>,
        minimum_backoff: Option<Duration>,
        escalate: bool,
    ) -> Result<ProviderStatus> {
        let mut status = self.status(provider_id).await?;
        status.record_failure(now, process_start, minimum_backoff, escalate);
        self.save_status(provider_id, &status).await?;
        Ok(status)
    }

    /// Providers of a kind that are blocked at `now`, with when they recover.
    pub async fn blocked(
        &self,
        kind: ProviderKind,
        now: DateTime<Utc>,
    ) -> Result<Vec<(i64, DateTime<Utc>)>> {
        let rows = sqlx::query(
            "SELECT s.provider_id, s.disabled_till FROM provider_status s
             JOIN providers p ON p.id = s.provider_id
             WHERE p.kind = ? AND s.disabled_till > ?",
        )
        .bind(kind.as_str())
        .bind(now)
        .fetch_all(self.0)
        .await?;
        rows.iter()
            .map(|r| Ok((r.try_get("provider_id")?, r.try_get("disabled_till")?)))
            .collect()
    }
}
