use sqlx::SqlitePool;

use crate::{DbError, Result};

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Tag {
    pub id: i64,
    pub label: String,
}

pub struct Tags<'a>(pub(crate) &'a SqlitePool);

impl Tags<'_> {
    pub async fn list(&self) -> Result<Vec<Tag>> {
        Ok(sqlx::query_as("SELECT id, label FROM tags ORDER BY label")
            .fetch_all(self.0)
            .await?)
    }

    /// Returns the tag with this label, creating it if needed. Labels are
    /// case-insensitive and stored lower-cased by the caller if it wants to.
    pub async fn get_or_create(&self, label: &str) -> Result<Tag> {
        let label = label.trim();
        if label.is_empty() {
            return Err(DbError::Constraint("a tag needs a label".into()));
        }
        sqlx::query("INSERT OR IGNORE INTO tags (label) VALUES (?)")
            .bind(label)
            .execute(self.0)
            .await?;
        Ok(sqlx::query_as("SELECT id, label FROM tags WHERE label = ?")
            .bind(label)
            .fetch_one(self.0)
            .await?)
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        let done = sqlx::query("DELETE FROM tags WHERE id = ?")
            .bind(id)
            .execute(self.0)
            .await?;
        if done.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct RootFolder {
    pub id: i64,
    pub path: String,
}

pub struct RootFolders<'a>(pub(crate) &'a SqlitePool);

impl RootFolders<'_> {
    pub async fn list(&self) -> Result<Vec<RootFolder>> {
        Ok(
            sqlx::query_as("SELECT id, path FROM root_folders ORDER BY path")
                .fetch_all(self.0)
                .await?,
        )
    }

    pub async fn add(&self, path: &str) -> Result<RootFolder> {
        let id = sqlx::query("INSERT INTO root_folders (path) VALUES (?)")
            .bind(path)
            .execute(self.0)
            .await?
            .last_insert_rowid();
        Ok(RootFolder {
            id,
            path: path.to_owned(),
        })
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        let done = sqlx::query("DELETE FROM root_folders WHERE id = ?")
            .bind(id)
            .execute(self.0)
            .await?;
        if done.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }
}
