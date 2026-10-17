use sqlx::SqlitePool;

use crate::Result;

/// Key/value settings.
pub struct Config<'a>(pub(crate) &'a SqlitePool);

impl Config<'_> {
    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(sqlx::query_scalar("SELECT value FROM config WHERE key = ?")
            .bind(key)
            .fetch_optional(self.0)
            .await?)
    }

    pub async fn set(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO config (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(self.0)
        .await?;
        Ok(())
    }

    pub async fn remove(&self, key: &str) -> Result<()> {
        sqlx::query("DELETE FROM config WHERE key = ?")
            .bind(key)
            .execute(self.0)
            .await?;
        Ok(())
    }

    pub async fn all(&self) -> Result<Vec<(String, String)>> {
        Ok(sqlx::query_as("SELECT key, value FROM config ORDER BY key")
            .fetch_all(self.0)
            .await?)
    }
}
