//! What Plex and Jellyfin servers hold, as read at their last sync.

use sqlx::{Row, SqlitePool};

use crate::Result;

/// One thing in a media server's library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaItem {
    /// The server's own id for it.
    pub key: String,
    /// "movie", "series" or "episode".
    pub kind: String,
    /// For an episode: the key of its series.
    pub series_key: Option<String>,
    pub title: String,
    pub year: Option<i64>,
    pub tmdb_id: Option<i64>,
    pub imdb_id: Option<String>,
    pub tvdb_id: Option<i64>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncStatus {
    pub provider_id: i64,
    pub synced_at: Option<String>,
    pub error: Option<String>,
    pub movies: i64,
    pub series: i64,
    pub episodes: i64,
}

pub struct MediaServerItems<'a>(pub(crate) &'a SqlitePool);

impl MediaServerItems<'_> {
    /// Replaces everything held for a server with `items`, all or nothing.
    pub async fn replace(&self, provider_id: i64, items: &[MediaItem]) -> Result<()> {
        let mut tx = self.0.begin().await?;
        sqlx::query("DELETE FROM media_server_items WHERE provider_id = ?")
            .bind(provider_id)
            .execute(&mut *tx)
            .await?;
        for i in items {
            // A server may list the same item twice (two libraries); the first wins.
            sqlx::query(
                "INSERT OR IGNORE INTO media_server_items
                 (provider_id, item_key, kind, series_key, title, year, tmdb_id, imdb_id, tvdb_id, season, episode)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(provider_id)
            .bind(&i.key)
            .bind(&i.kind)
            .bind(&i.series_key)
            .bind(&i.title)
            .bind(i.year)
            .bind(i.tmdb_id)
            .bind(&i.imdb_id)
            .bind(i.tvdb_id)
            .bind(i.season)
            .bind(i.episode)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Everything every server holds, with the provider each item is on.
    pub async fn all(&self) -> Result<Vec<(i64, MediaItem)>> {
        let rows = sqlx::query(
            "SELECT provider_id, item_key, kind, series_key, title, year, tmdb_id, imdb_id, tvdb_id, season, episode
             FROM media_server_items",
        )
        .fetch_all(self.0)
        .await?;
        rows.iter()
            .map(|r| {
                Ok((
                    r.try_get("provider_id")?,
                    MediaItem {
                        key: r.try_get("item_key")?,
                        kind: r.try_get("kind")?,
                        series_key: r.try_get("series_key")?,
                        title: r.try_get("title")?,
                        year: r.try_get("year")?,
                        tmdb_id: r.try_get("tmdb_id")?,
                        imdb_id: r.try_get("imdb_id")?,
                        tvdb_id: r.try_get("tvdb_id")?,
                        season: r.try_get("season")?,
                        episode: r.try_get("episode")?,
                    },
                ))
            })
            .collect()
    }

    pub async fn set_status(&self, s: &SyncStatus) -> Result<()> {
        sqlx::query(
            "INSERT INTO media_server_sync (provider_id, synced_at, error, movies, series, episodes)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(provider_id) DO UPDATE SET synced_at = excluded.synced_at, error = excluded.error,
               movies = excluded.movies, series = excluded.series, episodes = excluded.episodes",
        )
        .bind(s.provider_id)
        .bind(&s.synced_at)
        .bind(&s.error)
        .bind(s.movies)
        .bind(s.series)
        .bind(s.episodes)
        .execute(self.0)
        .await?;
        Ok(())
    }

    pub async fn statuses(&self) -> Result<Vec<SyncStatus>> {
        let rows = sqlx::query(
            "SELECT provider_id, synced_at, error, movies, series, episodes FROM media_server_sync",
        )
        .fetch_all(self.0)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(SyncStatus {
                    provider_id: r.try_get("provider_id")?,
                    synced_at: r.try_get("synced_at")?,
                    error: r.try_get("error")?,
                    movies: r.try_get("movies")?,
                    series: r.try_get("series")?,
                    episodes: r.try_get("episodes")?,
                })
            })
            .collect()
    }
}
