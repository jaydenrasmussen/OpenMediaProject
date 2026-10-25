use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};

use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub time: DateTime<Utc>,
    /// "error", "warn", "info" or "debug".
    pub level: String,
    pub logger: String,
    pub message: String,
}

fn rank(level: &str) -> i32 {
    match level {
        "error" => 4,
        "warn" => 3,
        "info" => 2,
        _ => 1,
    }
}

pub struct Logs<'a>(pub(crate) &'a SqlitePool);

impl Logs<'_> {
    pub async fn insert_batch(&self, entries: &[LogEntry]) -> Result<()> {
        let mut tx = self.0.begin().await?;
        for e in entries {
            sqlx::query("INSERT INTO logs (time, level, logger, message) VALUES (?, ?, ?, ?)")
                .bind(e.time)
                .bind(&e.level)
                .bind(&e.logger)
                .bind(&e.message)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Newest first. `min_level` keeps that level and anything worse;
    /// `search` matches the message or logger, case-insensitively.
    pub async fn recent(
        &self,
        min_level: &str,
        search: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<LogEntry>> {
        let levels: Vec<&str> = ["error", "warn", "info", "debug"]
            .into_iter()
            .filter(|l| rank(l) >= rank(min_level))
            .collect();
        let pattern = search.map(|s| format!("%{}%", s.replace('%', "\\%").replace('_', "\\_")));
        let placeholders = vec!["?"; levels.len()].join(",");
        let sql = format!(
            "SELECT time, level, logger, message FROM logs
             WHERE level IN ({placeholders})
               AND (? IS NULL OR message LIKE ? ESCAPE '\\' OR logger LIKE ? ESCAPE '\\')
             ORDER BY id DESC LIMIT ? OFFSET ?"
        );
        let mut q = sqlx::query(&sql);
        for l in &levels {
            q = q.bind(*l);
        }
        let rows = q
            .bind(&pattern)
            .bind(&pattern)
            .bind(&pattern)
            .bind(limit)
            .bind(offset)
            .fetch_all(self.0)
            .await?;
        rows.iter()
            .map(|r| {
                Ok(LogEntry {
                    time: r.try_get("time")?,
                    level: r.try_get("level")?,
                    logger: r.try_get("logger")?,
                    message: r.try_get("message")?,
                })
            })
            .collect()
    }

    /// Deletes entries older than `before`, then trims to the newest
    /// `keep` rows. Returns how many were removed.
    pub async fn purge(&self, before: DateTime<Utc>, keep: i64) -> Result<u64> {
        let old = sqlx::query("DELETE FROM logs WHERE time < ?")
            .bind(before)
            .execute(self.0)
            .await?
            .rows_affected();
        let extra = sqlx::query(
            "DELETE FROM logs WHERE id <= (SELECT id FROM logs ORDER BY id DESC LIMIT 1 OFFSET ?)",
        )
        .bind(keep)
        .execute(self.0)
        .await?
        .rows_affected();
        Ok(old + extra)
    }

    pub async fn clear(&self) -> Result<u64> {
        Ok(sqlx::query("DELETE FROM logs")
            .execute(self.0)
            .await?
            .rows_affected())
    }
}
