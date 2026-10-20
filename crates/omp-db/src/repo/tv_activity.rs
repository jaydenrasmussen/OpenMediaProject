//! Episode history, the TV blocklist and pending TV releases.

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

use super::PendingReason;
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
pub struct EpisodeHistoryRecord {
    pub id: i64,
    pub series_id: i64,
    pub episode_id: i64,
    pub event: HistoryEvent,
    pub date: DateTime<Utc>,
    pub source_title: String,
    pub quality: QualityModel,
    pub languages: Vec<Language>,
    pub download_id: Option<String>,
    pub data: BTreeMap<String, String>,
}

pub struct EpisodeHistory<'a>(pub(crate) &'a SqlitePool);

fn history_from_row(r: &SqliteRow) -> Result<EpisodeHistoryRecord> {
    Ok(EpisodeHistoryRecord {
        id: r.try_get("id")?,
        series_id: r.try_get("series_id")?,
        episode_id: r.try_get("episode_id")?,
        event: HistoryEvent::from_i32(r.try_get("event_type")?),
        date: r.try_get("date")?,
        source_title: r.try_get("source_title")?,
        quality: r.try_get::<Json<QualityModel>, _>("quality")?.0,
        languages: r.try_get::<Json<Vec<Language>>, _>("languages")?.0,
        download_id: r.try_get("download_id")?,
        data: r.try_get::<Json<BTreeMap<String, String>>, _>("data")?.0,
    })
}

impl EpisodeHistory<'_> {
    pub async fn insert(&self, h: &EpisodeHistoryRecord) -> Result<i64> {
        Ok(sqlx::query(
            "INSERT INTO episode_history (series_id, episode_id, event_type, date, source_title,
             quality, languages, download_id, data) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(h.series_id)
        .bind(h.episode_id)
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
    pub async fn for_episode(&self, episode_id: i64) -> Result<Vec<EpisodeHistoryRecord>> {
        sqlx::query(
            "SELECT * FROM episode_history WHERE episode_id = ? ORDER BY date DESC, id DESC",
        )
        .bind(episode_id)
        .fetch_all(self.0)
        .await?
        .iter()
        .map(history_from_row)
        .collect()
    }

    pub async fn for_series(&self, series_id: i64) -> Result<Vec<EpisodeHistoryRecord>> {
        sqlx::query("SELECT * FROM episode_history WHERE series_id = ? ORDER BY date DESC, id DESC")
            .bind(series_id)
            .fetch_all(self.0)
            .await?
            .iter()
            .map(history_from_row)
            .collect()
    }

    pub async fn recent(&self, limit: i64, offset: i64) -> Result<Vec<EpisodeHistoryRecord>> {
        sqlx::query("SELECT * FROM episode_history ORDER BY date DESC, id DESC LIMIT ? OFFSET ?")
            .bind(limit)
            .bind(offset)
            .fetch_all(self.0)
            .await?
            .iter()
            .map(history_from_row)
            .collect()
    }

    /// Every event that shares a download id, oldest first.
    pub async fn by_download_id(&self, download_id: &str) -> Result<Vec<EpisodeHistoryRecord>> {
        sqlx::query("SELECT * FROM episode_history WHERE download_id = ? ORDER BY date, id")
            .bind(download_id)
            .fetch_all(self.0)
            .await?
            .iter()
            .map(history_from_row)
            .collect()
    }
}

// ---- blocklist --------------------------------------------------------------------------------

/// A blocklisted release and the episodes it was for. `entry.movie_id` holds
/// the series id so the movie matching rules can be reused.
#[derive(Debug, Clone, PartialEq)]
pub struct TvBlocklistEntry {
    pub entry: BlocklistEntry,
    pub episode_ids: Vec<i64>,
}

pub struct TvBlocklist<'a>(pub(crate) &'a SqlitePool);

fn blocklist_from_row(r: &SqliteRow) -> Result<TvBlocklistEntry> {
    Ok(TvBlocklistEntry {
        entry: BlocklistEntry {
            id: r.try_get("id")?,
            movie_id: r.try_get("series_id")?,
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
        },
        episode_ids: r.try_get::<Json<Vec<i64>>, _>("episode_ids")?.0,
    })
}

impl TvBlocklist<'_> {
    pub async fn insert(&self, e: &TvBlocklistEntry) -> Result<i64> {
        let b = &e.entry;
        Ok(sqlx::query(
            "INSERT INTO tv_blocklist (series_id, episode_ids, source_title, quality, languages,
             published_date, size, protocol, indexer, indexer_flags, torrent_info_hash, message, date)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(b.movie_id)
        .bind(Json(&e.episode_ids))
        .bind(&b.source_title)
        .bind(Json(&b.quality))
        .bind(Json(&b.languages))
        .bind(b.published_date)
        .bind(b.size.map(|s| s as i64))
        .bind(protocol_value(b.protocol))
        .bind(&b.indexer)
        .bind(i64::from(b.indexer_flags.bits()))
        .bind(&b.torrent_info_hash)
        .bind(&b.message)
        .bind(b.date)
        .execute(self.0)
        .await?
        .last_insert_rowid())
    }

    pub async fn for_series(&self, series_id: i64) -> Result<Vec<TvBlocklistEntry>> {
        sqlx::query("SELECT * FROM tv_blocklist WHERE series_id = ? ORDER BY date DESC")
            .bind(series_id)
            .fetch_all(self.0)
            .await?
            .iter()
            .map(blocklist_from_row)
            .collect()
    }

    /// Newest first.
    pub async fn recent(&self, limit: i64, offset: i64) -> Result<Vec<TvBlocklistEntry>> {
        sqlx::query("SELECT * FROM tv_blocklist ORDER BY date DESC, id DESC LIMIT ? OFFSET ?")
            .bind(limit)
            .bind(offset)
            .fetch_all(self.0)
            .await?
            .iter()
            .map(blocklist_from_row)
            .collect()
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        let done = sqlx::query("DELETE FROM tv_blocklist WHERE id = ?")
            .bind(id)
            .execute(self.0)
            .await?;
        if done.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    pub async fn clear(&self) -> Result<u64> {
        Ok(sqlx::query("DELETE FROM tv_blocklist")
            .execute(self.0)
            .await?
            .rows_affected())
    }
}

// ---- pending releases ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct PendingTvRelease {
    pub id: i64,
    pub series_id: i64,
    pub episode_ids: Vec<i64>,
    pub added: DateTime<Utc>,
    pub release: ReleaseInfo,
    pub reason: PendingReason,
}

pub struct PendingTvReleases<'a>(pub(crate) &'a SqlitePool);

fn reason_value(r: PendingReason) -> i64 {
    match r {
        PendingReason::Delay => 0,
        PendingReason::DownloadClientUnavailable => 1,
        PendingReason::Fallback => 2,
    }
}

fn reason_from(v: i64) -> PendingReason {
    match v {
        1 => PendingReason::DownloadClientUnavailable,
        2 => PendingReason::Fallback,
        _ => PendingReason::Delay,
    }
}

fn pending_from_row(r: &SqliteRow) -> Result<PendingTvRelease> {
    Ok(PendingTvRelease {
        id: r.try_get("id")?,
        series_id: r.try_get("series_id")?,
        episode_ids: r.try_get::<Json<Vec<i64>>, _>("episode_ids")?.0,
        added: r.try_get("added")?,
        release: r.try_get::<Json<ReleaseInfo>, _>("release")?.0,
        reason: reason_from(r.try_get("reason")?),
    })
}

impl PendingTvReleases<'_> {
    pub async fn list(&self) -> Result<Vec<PendingTvRelease>> {
        sqlx::query("SELECT * FROM pending_tv_releases ORDER BY added")
            .fetch_all(self.0)
            .await?
            .iter()
            .map(pending_from_row)
            .collect()
    }

    pub async fn for_series(&self, series_id: i64) -> Result<Vec<PendingTvRelease>> {
        sqlx::query("SELECT * FROM pending_tv_releases WHERE series_id = ? ORDER BY added")
            .bind(series_id)
            .fetch_all(self.0)
            .await?
            .iter()
            .map(pending_from_row)
            .collect()
    }

    /// Adds a pending release unless the same one (title, publish date,
    /// indexer) already waits; a repeat may change the reason except that a
    /// release waiting on an unavailable client keeps it.
    pub async fn add_or_update(
        &self,
        series_id: i64,
        episode_ids: &[i64],
        release: &ReleaseInfo,
        reason: PendingReason,
    ) -> Result<i64> {
        let same: Vec<PendingTvRelease> = self
            .for_series(series_id)
            .await?
            .into_iter()
            .filter(|p| {
                p.release.title == release.title
                    && p.release.publish_date == release.publish_date
                    && p.release.indexer == release.indexer
            })
            .collect();
        let Some(first) = same.first() else {
            return Ok(sqlx::query(
                "INSERT INTO pending_tv_releases (series_id, episode_ids, title, added, release, reason)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(series_id)
            .bind(Json(episode_ids))
            .bind(&release.title)
            .bind(Utc::now())
            .bind(Json(release))
            .bind(reason_value(reason))
            .execute(self.0)
            .await?
            .last_insert_rowid());
        };
        if first.reason != reason && first.reason != PendingReason::DownloadClientUnavailable {
            sqlx::query("UPDATE pending_tv_releases SET reason = ? WHERE id = ?")
                .bind(reason_value(reason))
                .bind(first.id)
                .execute(self.0)
                .await?;
        }
        for extra in &same[1..] {
            self.remove(extra.id).await?;
        }
        Ok(first.id)
    }

    pub async fn remove(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM pending_tv_releases WHERE id = ?")
            .bind(id)
            .execute(self.0)
            .await?;
        Ok(())
    }
}
