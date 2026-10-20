//! History, blocklist and pending releases.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use omp_core::{
    blocklist::BlocklistEntry,
    history::HistoryEvent,
    language::Language,
    quality::QualityModel,
    release::{DownloadProtocol, IndexerFlags, ReleaseInfo},
};
use sqlx::{Row, SqlitePool, sqlite::SqliteRow, types::Json};

use crate::{DbError, Result};

fn protocol(value: i64) -> DownloadProtocol {
    match value {
        1 => DownloadProtocol::Usenet,
        2 => DownloadProtocol::Torrent,
        _ => DownloadProtocol::Unknown,
    }
}

fn protocol_value(p: DownloadProtocol) -> i64 {
    match p {
        DownloadProtocol::Unknown => 0,
        DownloadProtocol::Usenet => 1,
        DownloadProtocol::Torrent => 2,
    }
}

// ---- history ----------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRecord {
    pub id: i64,
    pub movie_id: i64,
    pub event: HistoryEvent,
    pub date: DateTime<Utc>,
    pub source_title: String,
    pub quality: QualityModel,
    pub languages: Vec<Language>,
    /// Torrent info hash (upper case) or the download client's id; joins a
    /// grab to its import or failure.
    pub download_id: Option<String>,
    /// Free-form details ("indexer", "size", "protocol", ...).
    pub data: BTreeMap<String, String>,
}

pub struct History<'a>(pub(crate) &'a SqlitePool);

fn history_from_row(r: &SqliteRow) -> Result<HistoryRecord> {
    Ok(HistoryRecord {
        id: r.try_get("id")?,
        movie_id: r.try_get("movie_id")?,
        event: HistoryEvent::from_i32(r.try_get("event_type")?),
        date: r.try_get("date")?,
        source_title: r.try_get("source_title")?,
        quality: r.try_get::<Json<QualityModel>, _>("quality")?.0,
        languages: r.try_get::<Json<Vec<Language>>, _>("languages")?.0,
        download_id: r.try_get("download_id")?,
        data: r.try_get::<Json<BTreeMap<String, String>>, _>("data")?.0,
    })
}

impl History<'_> {
    pub async fn insert(&self, h: &HistoryRecord) -> Result<i64> {
        Ok(sqlx::query(
            "INSERT INTO history (movie_id, event_type, date, source_title, quality, languages,
             download_id, data) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(h.movie_id)
        .bind(h.event.as_i32())
        .bind(h.date)
        .bind(&h.source_title)
        .bind(Json(&h.quality))
        .bind(Json(&h.languages))
        .bind(&h.download_id)
        .bind(Json(&h.data))
        .execute(self.0)
        .await?
        .last_insert_rowid())
    }

    /// Newest first.
    pub async fn for_movie(&self, movie_id: i64) -> Result<Vec<HistoryRecord>> {
        sqlx::query("SELECT * FROM history WHERE movie_id = ? ORDER BY date DESC, id DESC")
            .bind(movie_id)
            .fetch_all(self.0)
            .await?
            .iter()
            .map(history_from_row)
            .collect()
    }

    /// The most recent events across all movies, newest first.
    pub async fn recent(&self, limit: i64, offset: i64) -> Result<Vec<HistoryRecord>> {
        sqlx::query("SELECT * FROM history ORDER BY date DESC, id DESC LIMIT ? OFFSET ?")
            .bind(limit)
            .bind(offset)
            .fetch_all(self.0)
            .await?
            .iter()
            .map(history_from_row)
            .collect()
    }

    /// Events recorded after `id`, oldest first.
    pub async fn after(&self, id: i64, limit: i64) -> Result<Vec<HistoryRecord>> {
        sqlx::query("SELECT * FROM history WHERE id > ? ORDER BY id LIMIT ?")
            .bind(id)
            .bind(limit)
            .fetch_all(self.0)
            .await?
            .iter()
            .map(history_from_row)
            .collect()
    }

    /// Id of the newest event, 0 when there are none.
    pub async fn last_id(&self) -> Result<i64> {
        Ok(
            sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(id) FROM history")
                .fetch_one(self.0)
                .await?
                .unwrap_or(0),
        )
    }

    /// Every event that shares a download id, oldest first.
    pub async fn by_download_id(&self, download_id: &str) -> Result<Vec<HistoryRecord>> {
        sqlx::query("SELECT * FROM history WHERE download_id = ? ORDER BY date, id")
            .bind(download_id)
            .fetch_all(self.0)
            .await?
            .iter()
            .map(history_from_row)
            .collect()
    }

    pub async fn most_recent_for_movie(&self, movie_id: i64) -> Result<Option<HistoryRecord>> {
        sqlx::query("SELECT * FROM history WHERE movie_id = ? ORDER BY date DESC, id DESC LIMIT 1")
            .bind(movie_id)
            .fetch_optional(self.0)
            .await?
            .as_ref()
            .map(history_from_row)
            .transpose()
    }
}

// ---- blocklist --------------------------------------------------------------------------------

pub struct Blocklist<'a>(pub(crate) &'a SqlitePool);

fn blocklist_from_row(r: &SqliteRow) -> Result<BlocklistEntry> {
    Ok(BlocklistEntry {
        id: r.try_get("id")?,
        movie_id: r.try_get("movie_id")?,
        source_title: r.try_get("source_title")?,
        quality: r.try_get::<Json<QualityModel>, _>("quality")?.0,
        languages: r.try_get::<Json<Vec<Language>>, _>("languages")?.0,
        published_date: r.try_get("published_date")?,
        size: r.try_get::<Option<i64>, _>("size")?.map(|s| s as u64),
        protocol: protocol(r.try_get("protocol")?),
        indexer: r.try_get("indexer")?,
        indexer_flags: IndexerFlags(r.try_get::<i64, _>("indexer_flags")? as u32),
        torrent_info_hash: r.try_get("torrent_info_hash")?,
        message: r.try_get("message")?,
        date: r.try_get("date")?,
    })
}

impl Blocklist<'_> {
    pub async fn insert(&self, e: &BlocklistEntry) -> Result<i64> {
        Ok(sqlx::query(
            "INSERT INTO blocklist (movie_id, source_title, quality, languages, published_date, size,
             protocol, indexer, indexer_flags, torrent_info_hash, message, date)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(e.movie_id)
        .bind(&e.source_title)
        .bind(Json(&e.quality))
        .bind(Json(&e.languages))
        .bind(e.published_date)
        .bind(e.size.map(|s| s as i64))
        .bind(protocol_value(e.protocol))
        .bind(&e.indexer)
        .bind(i64::from(e.indexer_flags.bits()))
        .bind(&e.torrent_info_hash)
        .bind(&e.message)
        .bind(e.date)
        .execute(self.0)
        .await?
        .last_insert_rowid())
    }

    /// Newest first.
    pub async fn recent(&self, limit: i64, offset: i64) -> Result<Vec<BlocklistEntry>> {
        sqlx::query("SELECT * FROM blocklist ORDER BY date DESC, id DESC LIMIT ? OFFSET ?")
            .bind(limit)
            .bind(offset)
            .fetch_all(self.0)
            .await?
            .iter()
            .map(blocklist_from_row)
            .collect()
    }

    pub async fn for_movie(&self, movie_id: i64) -> Result<Vec<BlocklistEntry>> {
        sqlx::query("SELECT * FROM blocklist WHERE movie_id = ? ORDER BY date DESC")
            .bind(movie_id)
            .fetch_all(self.0)
            .await?
            .iter()
            .map(blocklist_from_row)
            .collect()
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        let done = sqlx::query("DELETE FROM blocklist WHERE id = ?")
            .bind(id)
            .execute(self.0)
            .await?;
        if done.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    pub async fn clear(&self) -> Result<u64> {
        Ok(sqlx::query("DELETE FROM blocklist")
            .execute(self.0)
            .await?
            .rows_affected())
    }
}

// ---- pending releases ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingReason {
    /// Waiting out a delay profile.
    Delay,
    /// No download client was available at grab time.
    DownloadClientUnavailable,
    /// A grab failed and this is the next candidate.
    Fallback,
}

impl PendingReason {
    fn as_i64(self) -> i64 {
        match self {
            PendingReason::Delay => 0,
            PendingReason::DownloadClientUnavailable => 1,
            PendingReason::Fallback => 2,
        }
    }

    fn from_i64(value: i64) -> PendingReason {
        match value {
            1 => PendingReason::DownloadClientUnavailable,
            2 => PendingReason::Fallback,
            _ => PendingReason::Delay,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingRelease {
    pub id: i64,
    pub movie_id: i64,
    pub added: DateTime<Utc>,
    pub release: ReleaseInfo,
    pub reason: PendingReason,
}

pub struct PendingReleases<'a>(pub(crate) &'a SqlitePool);

fn pending_from_row(r: &SqliteRow) -> Result<PendingRelease> {
    Ok(PendingRelease {
        id: r.try_get("id")?,
        movie_id: r.try_get("movie_id")?,
        added: r.try_get("added")?,
        release: r.try_get::<Json<ReleaseInfo>, _>("release")?.0,
        reason: PendingReason::from_i64(r.try_get("reason")?),
    })
}

impl PendingReleases<'_> {
    pub async fn add(
        &self,
        movie_id: i64,
        release: &ReleaseInfo,
        reason: PendingReason,
    ) -> Result<i64> {
        Ok(sqlx::query(
            "INSERT INTO pending_releases (movie_id, title, added, release, reason) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(movie_id)
        .bind(&release.title)
        .bind(Utc::now())
        .bind(Json(release))
        .bind(reason.as_i64())
        .execute(self.0)
        .await?
        .last_insert_rowid())
    }

    pub async fn list(&self) -> Result<Vec<PendingRelease>> {
        sqlx::query("SELECT * FROM pending_releases ORDER BY added")
            .fetch_all(self.0)
            .await?
            .iter()
            .map(pending_from_row)
            .collect()
    }

    pub async fn for_movie(&self, movie_id: i64) -> Result<Vec<PendingRelease>> {
        sqlx::query("SELECT * FROM pending_releases WHERE movie_id = ? ORDER BY added")
            .bind(movie_id)
            .fetch_all(self.0)
            .await?
            .iter()
            .map(pending_from_row)
            .collect()
    }

    /// Adds a pending release unless the same one is already waiting (same
    /// title, publish date and indexer). A repeat can change the reason, but a
    /// release waiting on an unavailable client keeps that reason. Extra
    /// duplicates are removed. Returns the id of the surviving row.
    pub async fn add_or_update(
        &self,
        movie_id: i64,
        release: &ReleaseInfo,
        reason: PendingReason,
    ) -> Result<i64> {
        let existing: Vec<PendingRelease> = self
            .for_movie(movie_id)
            .await?
            .into_iter()
            .filter(|p| {
                p.release.title == release.title
                    && p.release.publish_date == release.publish_date
                    && p.release.indexer == release.indexer
            })
            .collect();

        let Some(first) = existing.first() else {
            return self.add(movie_id, release, reason).await;
        };
        if first.reason != reason && first.reason != PendingReason::DownloadClientUnavailable {
            sqlx::query("UPDATE pending_releases SET reason = ? WHERE id = ?")
                .bind(reason.as_i64())
                .bind(first.id)
                .execute(self.0)
                .await?;
        }
        for duplicate in existing.iter().skip(1) {
            self.remove(duplicate.id).await?;
        }
        Ok(first.id)
    }

    pub async fn remove(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM pending_releases WHERE id = ?")
            .bind(id)
            .execute(self.0)
            .await?;
        Ok(())
    }
}
