use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::{DbError, Result};

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
}

pub struct Users<'a>(pub(crate) &'a SqlitePool);

impl Users<'_> {
    pub async fn count(&self) -> Result<i64> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(self.0)
            .await?)
    }

    /// Creates the first user, and only if there is none. Returns `None`
    /// when someone else got there first.
    pub async fn create_first(&self, username: &str, password_hash: &str) -> Result<Option<User>> {
        let done = sqlx::query(
            "INSERT INTO users (username, password_hash)
             SELECT ?, ? WHERE NOT EXISTS (SELECT 1 FROM users)",
        )
        .bind(username)
        .bind(password_hash)
        .execute(self.0)
        .await?;
        if done.rows_affected() == 0 {
            return Ok(None);
        }
        Ok(Some(
            self.by_username(username).await?.ok_or(DbError::NotFound)?,
        ))
    }

    pub async fn by_username(&self, username: &str) -> Result<Option<User>> {
        Ok(
            sqlx::query_as("SELECT id, username, password_hash FROM users WHERE username = ?")
                .bind(username)
                .fetch_optional(self.0)
                .await?,
        )
    }

    pub async fn get(&self, id: i64) -> Result<User> {
        sqlx::query_as("SELECT id, username, password_hash FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(self.0)
            .await?
            .ok_or(DbError::NotFound)
    }

    pub async fn set_password_hash(&self, id: i64, password_hash: &str) -> Result<()> {
        let done = sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
            .bind(password_hash)
            .bind(id)
            .execute(self.0)
            .await?;
        if done.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }
}

pub struct Sessions<'a>(pub(crate) &'a SqlitePool);

impl Sessions<'_> {
    pub async fn create(
        &self,
        token_hash: &str,
        user_id: i64,
        now: DateTime<Utc>,
        expires: DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO sessions (token_hash, user_id, created, expires) VALUES (?, ?, ?, ?)",
        )
        .bind(token_hash)
        .bind(user_id)
        .bind(now)
        .bind(expires)
        .execute(self.0)
        .await?;
        Ok(())
    }

    /// The user a live session belongs to.
    pub async fn user_for(&self, token_hash: &str, now: DateTime<Utc>) -> Result<Option<i64>> {
        Ok(
            sqlx::query_scalar("SELECT user_id FROM sessions WHERE token_hash = ? AND expires > ?")
                .bind(token_hash)
                .bind(now)
                .fetch_optional(self.0)
                .await?,
        )
    }

    pub async fn delete(&self, token_hash: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(token_hash)
            .execute(self.0)
            .await?;
        Ok(())
    }

    /// Ends every session of a user (after a password change, say).
    pub async fn delete_for_user(&self, user_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE user_id = ?")
            .bind(user_id)
            .execute(self.0)
            .await?;
        Ok(())
    }

    pub async fn purge_expired(&self, now: DateTime<Utc>) -> Result<u64> {
        Ok(sqlx::query("DELETE FROM sessions WHERE expires <= ?")
            .bind(now)
            .execute(self.0)
            .await?
            .rows_affected())
    }
}
