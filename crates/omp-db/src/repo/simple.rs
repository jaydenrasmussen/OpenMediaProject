use sqlx::{Row, SqlitePool, sqlite::SqliteRow, types::Json};

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

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ImportExclusion {
    pub tmdb_id: i64,
    pub title: String,
    pub year: i32,
}

pub struct ImportExclusions<'a>(pub(crate) &'a SqlitePool);

impl ImportExclusions<'_> {
    pub async fn list(&self) -> Result<Vec<ImportExclusion>> {
        Ok(sqlx::query_as(
            "SELECT tmdb_id, title, year FROM import_exclusions ORDER BY title COLLATE NOCASE",
        )
        .fetch_all(self.0)
        .await?)
    }

    /// Adding the same movie twice is fine.
    pub async fn add(&self, tmdb_id: i64, title: &str, year: i32) -> Result<()> {
        sqlx::query(
            "INSERT INTO import_exclusions (tmdb_id, title, year) VALUES (?, ?, ?)
             ON CONFLICT(tmdb_id) DO UPDATE SET title = excluded.title, year = excluded.year",
        )
        .bind(tmdb_id)
        .bind(title)
        .bind(year)
        .execute(self.0)
        .await?;
        Ok(())
    }

    pub async fn remove(&self, tmdb_id: i64) -> Result<()> {
        let done = sqlx::query("DELETE FROM import_exclusions WHERE tmdb_id = ?")
            .bind(tmdb_id)
            .execute(self.0)
            .await?;
        if done.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    pub async fn contains(&self, tmdb_id: i64) -> Result<bool> {
        Ok(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM import_exclusions WHERE tmdb_id = ?",
            )
            .bind(tmdb_id)
            .fetch_one(self.0)
            .await?
                > 0,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct RemotePathMapping {
    pub id: i64,
    /// The download client it applies to; `None` means every client.
    pub client_id: Option<i64>,
    pub remote_path: String,
    pub local_path: String,
}

pub struct RemotePathMappings<'a>(pub(crate) &'a SqlitePool);

impl RemotePathMappings<'_> {
    pub async fn list(&self) -> Result<Vec<RemotePathMapping>> {
        Ok(sqlx::query_as(
            "SELECT id, client_id, remote_path, local_path FROM remote_path_mappings ORDER BY id",
        )
        .fetch_all(self.0)
        .await?)
    }

    pub async fn insert(
        &self,
        client_id: Option<i64>,
        remote_path: &str,
        local_path: &str,
    ) -> Result<i64> {
        Ok(sqlx::query(
            "INSERT INTO remote_path_mappings (client_id, remote_path, local_path) VALUES (?, ?, ?)",
        )
        .bind(client_id)
        .bind(remote_path)
        .bind(local_path)
        .execute(self.0)
        .await?
        .last_insert_rowid())
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        let done = sqlx::query("DELETE FROM remote_path_mappings WHERE id = ?")
            .bind(id)
            .execute(self.0)
            .await?;
        if done.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }
}

/// Per-plugin key/value storage. Every call is scoped to one `plugin_id`, so
/// a plugin can only ever read or write its own keys.
pub struct PluginStorage<'a>(pub(crate) &'a SqlitePool);

impl PluginStorage<'_> {
    pub async fn get(&self, plugin_id: &str, key: &str) -> Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT value FROM plugin_storage WHERE plugin_id = ? AND key = ?")
                .bind(plugin_id)
                .bind(key)
                .fetch_optional(self.0)
                .await?,
        )
    }

    pub async fn set(&self, plugin_id: &str, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO plugin_storage (plugin_id, key, value) VALUES (?, ?, ?)
             ON CONFLICT(plugin_id, key) DO UPDATE SET value = excluded.value",
        )
        .bind(plugin_id)
        .bind(key)
        .bind(value)
        .execute(self.0)
        .await?;
        Ok(())
    }

    pub async fn delete(&self, plugin_id: &str, key: &str) -> Result<()> {
        sqlx::query("DELETE FROM plugin_storage WHERE plugin_id = ? AND key = ?")
            .bind(plugin_id)
            .bind(key)
            .execute(self.0)
            .await?;
        Ok(())
    }
}

/// A saved search/filter for a library view (e.g. "movies" or "series"),
/// matching Sonarr/Radarr's `CustomFilter`. `filters` is a list of free-form
/// filter rows; the database stores it as JSON and does not interpret it,
/// so the UI can evolve what a filter row looks like without a migration.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomFilter {
    pub id: i64,
    /// Which view this filter applies to, e.g. "movies" or "series".
    pub filter_type: String,
    pub label: String,
    pub filters: Vec<serde_json::Value>,
}

pub struct CustomFilters<'a>(pub(crate) &'a SqlitePool);

fn custom_filter_from_row(r: &SqliteRow) -> Result<CustomFilter> {
    Ok(CustomFilter {
        id: r.try_get("id")?,
        filter_type: r.try_get("type")?,
        label: r.try_get("label")?,
        filters: r.try_get::<Json<Vec<serde_json::Value>>, _>("filters")?.0,
    })
}

impl CustomFilters<'_> {
    /// All filters, or only those for one view type, newest first within
    /// their type is not guaranteed; ordered by label for a stable UI list.
    pub async fn list(&self, filter_type: Option<&str>) -> Result<Vec<CustomFilter>> {
        let rows = match filter_type {
            Some(t) => {
                sqlx::query(
                    "SELECT * FROM custom_filters WHERE type = ? ORDER BY label COLLATE NOCASE",
                )
                .bind(t)
                .fetch_all(self.0)
                .await?
            }
            None => {
                sqlx::query("SELECT * FROM custom_filters ORDER BY type, label COLLATE NOCASE")
                    .fetch_all(self.0)
                    .await?
            }
        };
        rows.iter().map(custom_filter_from_row).collect()
    }

    pub async fn get(&self, id: i64) -> Result<CustomFilter> {
        let row = sqlx::query("SELECT * FROM custom_filters WHERE id = ?")
            .bind(id)
            .fetch_optional(self.0)
            .await?
            .ok_or(DbError::NotFound)?;
        custom_filter_from_row(&row)
    }

    pub async fn insert(&self, f: &CustomFilter) -> Result<i64> {
        Ok(
            sqlx::query("INSERT INTO custom_filters (type, label, filters) VALUES (?, ?, ?)")
                .bind(&f.filter_type)
                .bind(&f.label)
                .bind(Json(&f.filters))
                .execute(self.0)
                .await?
                .last_insert_rowid(),
        )
    }

    pub async fn update(&self, f: &CustomFilter) -> Result<()> {
        let done =
            sqlx::query("UPDATE custom_filters SET type = ?, label = ?, filters = ? WHERE id = ?")
                .bind(&f.filter_type)
                .bind(&f.label)
                .bind(Json(&f.filters))
                .bind(f.id)
                .execute(self.0)
                .await?;
        if done.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        let done = sqlx::query("DELETE FROM custom_filters WHERE id = ?")
            .bind(id)
            .execute(self.0)
            .await?;
        if done.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }
}
