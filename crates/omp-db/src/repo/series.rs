//! Series, their seasons, episodes and episode files.

use chrono::{DateTime, NaiveDate, Utc};
use omp_core::{
    language::Language,
    movie::{AlternativeTitle, MediaCover, Rating},
    quality::QualityModel,
    release::IndexerFlags,
    tv::{EpisodeInfo, NewItemMonitor, SeriesInfo, SeriesStatus, SeriesType},
};
use omp_parser::{normalize::normalize_title, tv::clean_series_title};
use sqlx::{Row, SqlitePool, sqlite::SqliteRow, types::Json};

use crate::{DbError, Result};

// ---- series ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct SeasonRecord {
    pub season_number: i32,
    pub monitored: bool,
    pub title: Option<String>,
    pub images: Vec<MediaCover>,
}

/// A library series: provider metadata plus the choices made about it.
#[derive(Debug, Clone, PartialEq)]
pub struct SeriesRecord {
    pub id: i64,
    /// Provider metadata. `episodes` is empty here; see [`Episodes`].
    pub info: SeriesInfo,
    pub series_type: SeriesType,
    pub monitored: bool,
    pub monitor_new_items: NewItemMonitor,
    pub quality_profile_id: i32,
    pub season_folder: bool,
    pub use_scene_numbering: bool,
    pub path: String,
    pub root_folder_path: Option<String>,
    pub tags: Vec<i32>,
    pub added: DateTime<Utc>,
    pub last_info_sync: Option<DateTime<Utc>>,
    pub seasons: Vec<SeasonRecord>,
}

impl SeriesRecord {
    pub fn title(&self) -> &str {
        &self.info.title
    }

    pub fn season(&self, number: i32) -> Option<&SeasonRecord> {
        self.seasons.iter().find(|s| s.season_number == number)
    }
}

#[derive(Debug, Clone)]
pub struct NewSeries {
    pub info: SeriesInfo,
    pub series_type: SeriesType,
    pub monitored: bool,
    pub monitor_new_items: NewItemMonitor,
    pub quality_profile_id: i32,
    pub season_folder: bool,
    pub use_scene_numbering: bool,
    pub path: String,
    pub root_folder_path: Option<String>,
    pub tags: Vec<i32>,
}

/// The fields a user can edit.
#[derive(Debug, Clone)]
pub struct SeriesUpdate {
    pub series_type: SeriesType,
    pub monitored: bool,
    pub monitor_new_items: NewItemMonitor,
    pub quality_profile_id: i32,
    pub season_folder: bool,
    pub use_scene_numbering: bool,
    pub path: String,
    pub root_folder_path: Option<String>,
    pub tags: Vec<i32>,
}

pub struct Series<'a>(pub(crate) &'a SqlitePool);

fn monitor_new(n: i64) -> NewItemMonitor {
    if n == 1 {
        NewItemMonitor::None
    } else {
        NewItemMonitor::All
    }
}

fn series_from_row(r: &SqliteRow) -> Result<SeriesRecord> {
    let votes: Option<i64> = r.try_get("rating_votes")?;
    let value: Option<f64> = r.try_get("rating_value")?;
    let series_type = SeriesType::from_i32(r.try_get::<i64, _>("series_type")? as i32);
    let info = SeriesInfo {
        tmdb_id: r.try_get("tmdb_id")?,
        tvdb_id: r.try_get("tvdb_id")?,
        imdb_id: r.try_get("imdb_id")?,
        title: r.try_get("title")?,
        original_title: r.try_get("original_title")?,
        original_language: r.try_get("original_language")?,
        overview: r.try_get("overview")?,
        year: r.try_get("year")?,
        status: SeriesStatus::from_i32(r.try_get::<i64, _>("status")? as i32),
        network: r.try_get("network")?,
        runtime: r.try_get::<i64, _>("runtime")? as u32,
        genres: r.try_get::<Json<Vec<String>>, _>("genres")?.0,
        certification: r.try_get("certification")?,
        first_aired: r.try_get("first_aired")?,
        last_aired: r.try_get("last_aired")?,
        images: r.try_get::<Json<Vec<MediaCover>>, _>("images")?.0,
        alternative_titles: r
            .try_get::<Json<Vec<AlternativeTitle>>, _>("alternative_titles")?
            .0,
        ratings: votes.zip(value).map(|(v, value)| Rating {
            votes: v as u32,
            value,
        }),
        suggested_type: series_type,
        seasons: Vec::new(),
        episodes: Vec::new(),
    };
    Ok(SeriesRecord {
        id: r.try_get("id")?,
        info,
        series_type,
        monitored: r.try_get("monitored")?,
        monitor_new_items: monitor_new(r.try_get("monitor_new_items")?),
        quality_profile_id: r.try_get("quality_profile_id")?,
        season_folder: r.try_get("season_folder")?,
        use_scene_numbering: r.try_get("use_scene_numbering")?,
        path: r.try_get("path")?,
        root_folder_path: r.try_get("root_folder_path")?,
        tags: r.try_get::<Json<Vec<i32>>, _>("tags")?.0,
        added: r.try_get("added")?,
        last_info_sync: r.try_get("last_info_sync")?,
        seasons: Vec::new(),
    })
}

fn season_from_row(r: &SqliteRow) -> Result<SeasonRecord> {
    Ok(SeasonRecord {
        season_number: r.try_get("season_number")?,
        monitored: r.try_get("monitored")?,
        title: r.try_get("title")?,
        images: r.try_get::<Json<Vec<MediaCover>>, _>("images")?.0,
    })
}

impl Series<'_> {
    async fn with_seasons(&self, mut series: Vec<SeriesRecord>) -> Result<Vec<SeriesRecord>> {
        let rows = sqlx::query("SELECT * FROM seasons ORDER BY series_id, season_number")
            .fetch_all(self.0)
            .await?;
        for row in &rows {
            let series_id: i64 = row.try_get("series_id")?;
            if let Some(s) = series.iter_mut().find(|s| s.id == series_id) {
                s.seasons.push(season_from_row(row)?);
            }
        }
        Ok(series)
    }

    pub async fn list(&self) -> Result<Vec<SeriesRecord>> {
        let rows = sqlx::query("SELECT * FROM series ORDER BY sort_title")
            .fetch_all(self.0)
            .await?;
        let series = rows
            .iter()
            .map(series_from_row)
            .collect::<Result<Vec<_>>>()?;
        self.with_seasons(series).await
    }

    pub async fn get(&self, id: i64) -> Result<SeriesRecord> {
        let row = sqlx::query("SELECT * FROM series WHERE id = ?")
            .bind(id)
            .fetch_optional(self.0)
            .await?
            .ok_or(DbError::NotFound)?;
        let series = vec![series_from_row(&row)?];
        Ok(self.with_seasons(series).await?.remove(0))
    }

    pub async fn find_by_tmdb_id(&self, tmdb_id: i64) -> Result<Option<SeriesRecord>> {
        let id: Option<i64> = sqlx::query_scalar("SELECT id FROM series WHERE tmdb_id = ?")
            .bind(tmdb_id)
            .fetch_optional(self.0)
            .await?;
        match id {
            Some(id) => Ok(Some(self.get(id).await?)),
            None => Ok(None),
        }
    }

    pub async fn find_by_tvdb_id(&self, tvdb_id: i64) -> Result<Option<SeriesRecord>> {
        let id: Option<i64> = sqlx::query_scalar("SELECT id FROM series WHERE tvdb_id = ?")
            .bind(tvdb_id)
            .fetch_optional(self.0)
            .await?;
        match id {
            Some(id) => Ok(Some(self.get(id).await?)),
            None => Ok(None),
        }
    }

    /// Series whose cleaned title (or an alternative title) is `clean_title`.
    pub async fn find_by_clean_title(&self, clean_title: &str) -> Result<Vec<SeriesRecord>> {
        Ok(self
            .list()
            .await?
            .into_iter()
            .filter(|s| {
                clean_series_title(&s.info.title) == clean_title
                    || s.info
                        .alternative_titles
                        .iter()
                        .any(|t| clean_series_title(&t.title) == clean_title)
                    || (!s.info.original_title.is_empty()
                        && clean_series_title(&s.info.original_title) == clean_title)
            })
            .collect())
    }

    pub async fn add(&self, n: &NewSeries) -> Result<i64> {
        let i = &n.info;
        let id = sqlx::query(
            "INSERT INTO series (tmdb_id, tvdb_id, imdb_id, title, clean_title, sort_title,
             original_title, original_language, overview, year, status, network, runtime, genres,
             certification, first_aired, last_aired, images, alternative_titles, rating_votes,
             rating_value, series_type, monitored, monitor_new_items, quality_profile_id,
             season_folder, use_scene_numbering, path, root_folder_path, tags, added, last_info_sync)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(i.tmdb_id)
        .bind(i.tvdb_id)
        .bind(&i.imdb_id)
        .bind(&i.title)
        .bind(clean_series_title(&i.title))
        .bind(normalize_title(&i.title))
        .bind(&i.original_title)
        .bind(&i.original_language)
        .bind(&i.overview)
        .bind(i.year)
        .bind(i.status.as_i32())
        .bind(&i.network)
        .bind(i64::from(i.runtime))
        .bind(Json(&i.genres))
        .bind(&i.certification)
        .bind(i.first_aired)
        .bind(i.last_aired)
        .bind(Json(&i.images))
        .bind(Json(&i.alternative_titles))
        .bind(i.ratings.as_ref().map(|r| i64::from(r.votes)))
        .bind(i.ratings.as_ref().map(|r| r.value))
        .bind(n.series_type.as_i32())
        .bind(n.monitored)
        .bind(i32::from(n.monitor_new_items == NewItemMonitor::None))
        .bind(n.quality_profile_id)
        .bind(n.season_folder)
        .bind(n.use_scene_numbering)
        .bind(&n.path)
        .bind(&n.root_folder_path)
        .bind(Json(&n.tags))
        .bind(Utc::now())
        .bind(Option::<DateTime<Utc>>::None)
        .execute(self.0)
        .await?
        .last_insert_rowid();
        Ok(id)
    }

    pub async fn update(&self, id: i64, u: &SeriesUpdate) -> Result<()> {
        let done = sqlx::query(
            "UPDATE series SET series_type = ?, monitored = ?, monitor_new_items = ?,
             quality_profile_id = ?, season_folder = ?, use_scene_numbering = ?, path = ?,
             root_folder_path = ?, tags = ? WHERE id = ?",
        )
        .bind(u.series_type.as_i32())
        .bind(u.monitored)
        .bind(i32::from(u.monitor_new_items == NewItemMonitor::None))
        .bind(u.quality_profile_id)
        .bind(u.season_folder)
        .bind(u.use_scene_numbering)
        .bind(&u.path)
        .bind(&u.root_folder_path)
        .bind(Json(&u.tags))
        .bind(id)
        .execute(self.0)
        .await?;
        if done.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    /// Stores fresh provider metadata without touching the user's choices.
    pub async fn apply_info(&self, id: i64, i: &SeriesInfo) -> Result<()> {
        let done = sqlx::query(
            "UPDATE series SET tvdb_id = ?, imdb_id = ?, title = ?, clean_title = ?, sort_title = ?,
             original_title = ?, original_language = ?, overview = ?, year = ?, status = ?,
             network = ?, runtime = ?, genres = ?, certification = ?, first_aired = ?, last_aired = ?,
             images = ?, alternative_titles = ?, rating_votes = ?, rating_value = ?, last_info_sync = ?
             WHERE id = ?",
        )
        .bind(i.tvdb_id)
        .bind(&i.imdb_id)
        .bind(&i.title)
        .bind(clean_series_title(&i.title))
        .bind(normalize_title(&i.title))
        .bind(&i.original_title)
        .bind(&i.original_language)
        .bind(&i.overview)
        .bind(i.year)
        .bind(i.status.as_i32())
        .bind(&i.network)
        .bind(i64::from(i.runtime))
        .bind(Json(&i.genres))
        .bind(&i.certification)
        .bind(i.first_aired)
        .bind(i.last_aired)
        .bind(Json(&i.images))
        .bind(Json(&i.alternative_titles))
        .bind(i.ratings.as_ref().map(|r| i64::from(r.votes)))
        .bind(i.ratings.as_ref().map(|r| r.value))
        .bind(Utc::now())
        .bind(id)
        .execute(self.0)
        .await?;
        if done.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    /// Replaces the series' seasons. Seasons that already exist keep their
    /// monitored flag unless `monitored` says what a new season should be.
    pub async fn set_seasons(&self, id: i64, seasons: &[SeasonRecord]) -> Result<()> {
        let mut tx = self.0.begin().await?;
        sqlx::query("DELETE FROM seasons WHERE series_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        for s in seasons {
            sqlx::query(
                "INSERT INTO seasons (series_id, season_number, monitored, title, images)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(s.season_number)
            .bind(s.monitored)
            .bind(&s.title)
            .bind(Json(&s.images))
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_season_monitored(
        &self,
        id: i64,
        season_number: i32,
        monitored: bool,
    ) -> Result<()> {
        let done = sqlx::query(
            "UPDATE seasons SET monitored = ? WHERE series_id = ? AND season_number = ?",
        )
        .bind(monitored)
        .bind(id)
        .bind(season_number)
        .execute(self.0)
        .await?;
        if done.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        let done = sqlx::query("DELETE FROM series WHERE id = ?")
            .bind(id)
            .execute(self.0)
            .await?;
        if done.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }
}

// ---- episodes -------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct EpisodeRecord {
    pub id: i64,
    pub series_id: i64,
    pub tmdb_id: i64,
    pub season_number: i32,
    pub episode_number: i32,
    pub absolute_episode_number: Option<i32>,
    pub scene_season_number: Option<i32>,
    pub scene_episode_number: Option<i32>,
    pub scene_absolute_episode_number: Option<i32>,
    pub title: String,
    pub overview: Option<String>,
    pub air_date: Option<NaiveDate>,
    pub air_date_utc: Option<DateTime<Utc>>,
    pub runtime: u32,
    pub monitored: bool,
    pub episode_file_id: Option<i64>,
    pub last_search_time: Option<DateTime<Utc>>,
    pub finale_type: Option<String>,
}

impl EpisodeRecord {
    pub fn has_file(&self) -> bool {
        self.episode_file_id.is_some()
    }

    /// Has it aired (by the provider's date) at `now`?
    pub fn has_aired(&self, now: DateTime<Utc>) -> bool {
        self.air_date_utc.is_some_and(|d| d <= now)
    }
}

fn episode_from_row(r: &SqliteRow) -> Result<EpisodeRecord> {
    Ok(EpisodeRecord {
        id: r.try_get("id")?,
        series_id: r.try_get("series_id")?,
        tmdb_id: r.try_get("tmdb_id")?,
        season_number: r.try_get("season_number")?,
        episode_number: r.try_get("episode_number")?,
        absolute_episode_number: r.try_get("absolute_episode_number")?,
        scene_season_number: r.try_get("scene_season_number")?,
        scene_episode_number: r.try_get("scene_episode_number")?,
        scene_absolute_episode_number: r.try_get("scene_absolute_episode_number")?,
        title: r.try_get("title")?,
        overview: r.try_get("overview")?,
        air_date: r.try_get("air_date")?,
        air_date_utc: r.try_get("air_date_utc")?,
        runtime: r.try_get::<i64, _>("runtime")? as u32,
        monitored: r.try_get("monitored")?,
        episode_file_id: r.try_get("episode_file_id")?,
        last_search_time: r.try_get("last_search_time")?,
        finale_type: r.try_get("finale_type")?,
    })
}

pub struct Episodes<'a>(pub(crate) &'a SqlitePool);

impl Episodes<'_> {
    /// In season and episode order.
    pub async fn for_series(&self, series_id: i64) -> Result<Vec<EpisodeRecord>> {
        sqlx::query(
            "SELECT * FROM episodes WHERE series_id = ? ORDER BY season_number, episode_number",
        )
        .bind(series_id)
        .fetch_all(self.0)
        .await?
        .iter()
        .map(episode_from_row)
        .collect()
    }

    pub async fn get(&self, id: i64) -> Result<EpisodeRecord> {
        sqlx::query("SELECT * FROM episodes WHERE id = ?")
            .bind(id)
            .fetch_optional(self.0)
            .await?
            .as_ref()
            .map(episode_from_row)
            .transpose()?
            .ok_or(DbError::NotFound)
    }

    pub async fn get_many(&self, ids: &[i64]) -> Result<Vec<EpisodeRecord>> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            out.push(self.get(*id).await?);
        }
        Ok(out)
    }

    pub async fn for_file(&self, episode_file_id: i64) -> Result<Vec<EpisodeRecord>> {
        sqlx::query(
            "SELECT * FROM episodes WHERE episode_file_id = ? ORDER BY season_number, episode_number",
        )
        .bind(episode_file_id)
        .fetch_all(self.0)
        .await?
        .iter()
        .map(episode_from_row)
        .collect()
    }

    pub async fn find(
        &self,
        series_id: i64,
        season_number: i32,
        episode_number: i32,
    ) -> Result<Option<EpisodeRecord>> {
        sqlx::query(
            "SELECT * FROM episodes WHERE series_id = ? AND season_number = ? AND episode_number = ?",
        )
        .bind(series_id)
        .bind(season_number)
        .bind(episode_number)
        .fetch_optional(self.0)
        .await?
        .as_ref()
        .map(episode_from_row)
        .transpose()
    }

    /// Every episode of a series that aired on `date`.
    pub async fn aired_on(&self, series_id: i64, date: NaiveDate) -> Result<Vec<EpisodeRecord>> {
        sqlx::query(
            "SELECT * FROM episodes WHERE series_id = ? AND air_date = ? ORDER BY season_number, episode_number",
        )
        .bind(series_id)
        .bind(date)
        .fetch_all(self.0)
        .await?
        .iter()
        .map(episode_from_row)
        .collect()
    }

    /// Adds an episode from provider data. Returns its id.
    pub async fn insert(
        &self,
        series_id: i64,
        e: &EpisodeInfo,
        absolute: Option<i32>,
        monitored: bool,
    ) -> Result<i64> {
        Ok(sqlx::query(
            "INSERT INTO episodes (series_id, tmdb_id, season_number, episode_number,
             absolute_episode_number, title, overview, air_date, air_date_utc, runtime, monitored,
             finale_type) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(series_id)
        .bind(e.tmdb_id)
        .bind(e.season_number)
        .bind(e.episode_number)
        .bind(absolute)
        .bind(&e.title)
        .bind(&e.overview)
        .bind(e.air_date)
        .bind(e.air_date_utc)
        .bind(i64::from(e.runtime))
        .bind(monitored)
        .bind(&e.finale_type)
        .execute(self.0)
        .await?
        .last_insert_rowid())
    }

    /// Updates the provider-owned fields of an episode; the monitored flag
    /// and the file are left alone.
    pub async fn apply_info(&self, id: i64, e: &EpisodeInfo, absolute: Option<i32>) -> Result<()> {
        sqlx::query(
            "UPDATE episodes SET tmdb_id = ?, absolute_episode_number = ?, title = ?, overview = ?,
             air_date = ?, air_date_utc = ?, runtime = ?, finale_type = ? WHERE id = ?",
        )
        .bind(e.tmdb_id)
        .bind(absolute)
        .bind(&e.title)
        .bind(&e.overview)
        .bind(e.air_date)
        .bind(e.air_date_utc)
        .bind(i64::from(e.runtime))
        .bind(&e.finale_type)
        .bind(id)
        .execute(self.0)
        .await?;
        Ok(())
    }

    pub async fn delete_many(&self, ids: &[i64]) -> Result<()> {
        for id in ids {
            sqlx::query("DELETE FROM episodes WHERE id = ?")
                .bind(id)
                .execute(self.0)
                .await?;
        }
        Ok(())
    }

    pub async fn set_monitored(&self, ids: &[i64], monitored: bool) -> Result<()> {
        let mut tx = self.0.begin().await?;
        for id in ids {
            sqlx::query("UPDATE episodes SET monitored = ? WHERE id = ?")
                .bind(monitored)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_last_search_time(&self, ids: &[i64], when: DateTime<Utc>) -> Result<()> {
        let mut tx = self.0.begin().await?;
        for id in ids {
            sqlx::query("UPDATE episodes SET last_search_time = ? WHERE id = ?")
                .bind(when)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_scene_numbers(
        &self,
        id: i64,
        season: Option<i32>,
        episode: Option<i32>,
        absolute: Option<i32>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE episodes SET scene_season_number = ?, scene_episode_number = ?,
             scene_absolute_episode_number = ? WHERE id = ?",
        )
        .bind(season)
        .bind(episode)
        .bind(absolute)
        .bind(id)
        .execute(self.0)
        .await?;
        Ok(())
    }

    /// Points episodes at a file (or at none).
    pub async fn set_file(&self, ids: &[i64], file_id: Option<i64>) -> Result<()> {
        let mut tx = self.0.begin().await?;
        for id in ids {
            sqlx::query("UPDATE episodes SET episode_file_id = ? WHERE id = ?")
                .bind(file_id)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

// ---- episode files ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct EpisodeFileRecord {
    pub id: i64,
    pub series_id: i64,
    pub season_number: i32,
    pub relative_path: String,
    pub size: u64,
    pub date_added: DateTime<Utc>,
    pub scene_name: Option<String>,
    pub release_group: Option<String>,
    pub quality: QualityModel,
    pub languages: Vec<Language>,
    pub media_info: Option<serde_json::Value>,
    pub original_file_path: Option<String>,
    pub indexer_flags: IndexerFlags,
}

fn file_from_row(r: &SqliteRow) -> Result<EpisodeFileRecord> {
    Ok(EpisodeFileRecord {
        id: r.try_get("id")?,
        series_id: r.try_get("series_id")?,
        season_number: r.try_get("season_number")?,
        relative_path: r.try_get("relative_path")?,
        size: r.try_get::<i64, _>("size")? as u64,
        date_added: r.try_get("date_added")?,
        scene_name: r.try_get("scene_name")?,
        release_group: r.try_get("release_group")?,
        quality: r.try_get::<Json<QualityModel>, _>("quality")?.0,
        languages: r.try_get::<Json<Vec<Language>>, _>("languages")?.0,
        media_info: r
            .try_get::<Option<Json<serde_json::Value>>, _>("media_info")?
            .map(|j| j.0),
        original_file_path: r.try_get("original_file_path")?,
        indexer_flags: IndexerFlags(r.try_get::<i64, _>("indexer_flags")? as u32),
    })
}

pub struct EpisodeFiles<'a>(pub(crate) &'a SqlitePool);

impl EpisodeFiles<'_> {
    /// Inserts a file. The record's `id` is ignored; the new id is returned.
    pub async fn insert(&self, f: &EpisodeFileRecord) -> Result<i64> {
        Ok(sqlx::query(
            "INSERT INTO episode_files (series_id, season_number, relative_path, size, date_added,
             scene_name, release_group, quality, languages, media_info, original_file_path,
             indexer_flags) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(f.series_id)
        .bind(f.season_number)
        .bind(&f.relative_path)
        .bind(f.size as i64)
        .bind(f.date_added)
        .bind(&f.scene_name)
        .bind(&f.release_group)
        .bind(Json(&f.quality))
        .bind(Json(&f.languages))
        .bind(f.media_info.as_ref().map(Json))
        .bind(&f.original_file_path)
        .bind(i64::from(f.indexer_flags.bits()))
        .execute(self.0)
        .await?
        .last_insert_rowid())
    }

    pub async fn get(&self, id: i64) -> Result<EpisodeFileRecord> {
        sqlx::query("SELECT * FROM episode_files WHERE id = ?")
            .bind(id)
            .fetch_optional(self.0)
            .await?
            .as_ref()
            .map(file_from_row)
            .transpose()?
            .ok_or(DbError::NotFound)
    }

    pub async fn for_series(&self, series_id: i64) -> Result<Vec<EpisodeFileRecord>> {
        sqlx::query(
            "SELECT * FROM episode_files WHERE series_id = ? ORDER BY season_number, relative_path",
        )
        .bind(series_id)
        .fetch_all(self.0)
        .await?
        .iter()
        .map(file_from_row)
        .collect()
    }

    pub async fn update_path(&self, id: i64, relative_path: &str) -> Result<()> {
        sqlx::query("UPDATE episode_files SET relative_path = ? WHERE id = ?")
            .bind(relative_path)
            .bind(id)
            .execute(self.0)
            .await?;
        Ok(())
    }

    /// Removes the file row; the episodes that pointed at it keep existing
    /// and lose their file (the foreign key sets it to null).
    pub async fn delete(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM episode_files WHERE id = ?")
            .bind(id)
            .execute(self.0)
            .await?;
        Ok(())
    }
}
