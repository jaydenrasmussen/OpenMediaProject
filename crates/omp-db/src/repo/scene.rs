//! Alternate scene titles for series.

use sqlx::{Row, SqlitePool};

use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneTitleRow {
    pub tvdb_id: i64,
    pub title: String,
    pub search_title: String,
    pub season: i32,
}

pub struct SceneTitles<'a>(pub(crate) &'a SqlitePool);

impl SceneTitles<'_> {
    /// Swaps the whole list in one go.
    pub async fn replace_all(&self, rows: &[SceneTitleRow]) -> Result<()> {
        let mut tx = self.0.begin().await?;
        sqlx::query("DELETE FROM scene_titles")
            .execute(&mut *tx)
            .await?;
        for r in rows {
            sqlx::query(
                "INSERT OR IGNORE INTO scene_titles (tvdb_id, title, search_title, season)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(r.tvdb_id)
            .bind(&r.title)
            .bind(&r.search_title)
            .bind(r.season)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn for_tvdb_id(&self, tvdb_id: i64) -> Result<Vec<SceneTitleRow>> {
        Ok(
            sqlx::query("SELECT * FROM scene_titles WHERE tvdb_id = ? ORDER BY title")
                .bind(tvdb_id)
                .fetch_all(self.0)
                .await?
                .iter()
                .map(|r| SceneTitleRow {
                    tvdb_id: r.get("tvdb_id"),
                    title: r.get("title"),
                    search_title: r.get("search_title"),
                    season: r.get("season"),
                })
                .collect(),
        )
    }

    pub async fn count(&self) -> Result<i64> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM scene_titles")
            .fetch_one(self.0)
            .await?)
    }
}
