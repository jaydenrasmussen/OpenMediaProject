use chrono::{DateTime, Utc};
use omp_core::{
    language::Language,
    library::{ExistingFile, LibraryMovie},
    movie::{AlternativeTitle, Credit, MediaCover, MovieMetadata, MovieStatus, Rating},
    quality::QualityModel,
    release::IndexerFlags,
};
use omp_parser::normalize::{clean_movie_title, normalize_title};
use sqlx::{Row, SqlitePool, sqlite::SqliteRow, types::Json};

use crate::{DbError, Result};

fn status(value: i32) -> MovieStatus {
    MovieStatus::from_i32(value).unwrap_or(MovieStatus::Tba)
}

// ---- movies and their metadata -------------------------------------------------------------

/// A library movie joined with its provider metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct MovieRecord {
    pub movie: LibraryMovie,
    pub metadata: MovieMetadata,
    pub metadata_id: i64,
    pub root_folder_path: Option<String>,
    pub added: DateTime<Utc>,
    pub last_search_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewMovie {
    pub metadata_id: i64,
    pub monitored: bool,
    pub minimum_availability: MovieStatus,
    pub quality_profile_id: i32,
    pub path: String,
    pub root_folder_path: Option<String>,
    pub tags: Vec<i32>,
}

/// The fields a user can edit.
#[derive(Debug, Clone)]
pub struct MovieUpdate {
    pub monitored: bool,
    pub minimum_availability: MovieStatus,
    pub quality_profile_id: i32,
    pub path: String,
    pub root_folder_path: Option<String>,
    pub tags: Vec<i32>,
}

pub struct Movies<'a>(pub(crate) &'a SqlitePool);

const SELECT_MOVIE: &str = "SELECT m.id AS movie_id, m.monitored, m.minimum_availability,
        m.quality_profile_id, m.path, m.root_folder_path, m.added, m.tags, m.last_search_time,
        md.* FROM movies m JOIN movie_metadata md ON md.id = m.movie_metadata_id";

fn metadata_from_row(r: &SqliteRow) -> Result<MovieMetadata> {
    let votes: Option<i64> = r.try_get("rating_votes")?;
    let value: Option<f64> = r.try_get("rating_value")?;
    Ok(MovieMetadata {
        tmdb_id: r.try_get("tmdb_id")?,
        imdb_id: r.try_get("imdb_id")?,
        title: r.try_get("title")?,
        original_title: r.try_get("original_title")?,
        original_language: r.try_get("original_language")?,
        overview: r.try_get("overview")?,
        year: r.try_get("year")?,
        runtime: r.try_get::<i64, _>("runtime")? as u32,
        status: status(r.try_get("status")?),
        certification: r.try_get("certification")?,
        genres: r.try_get::<Json<Vec<String>>, _>("genres")?.0,
        in_cinemas: r.try_get("in_cinemas")?,
        physical_release: r.try_get("physical_release")?,
        digital_release: r.try_get("digital_release")?,
        collection_tmdb_id: r.try_get("collection_tmdb_id")?,
        collection_title: r.try_get("collection_title")?,
        website: r.try_get("website")?,
        studio: r.try_get("studio")?,
        ratings: votes.zip(value).map(|(v, value)| Rating {
            votes: v as u32,
            value,
        }),
        images: r.try_get::<Json<Vec<MediaCover>>, _>("images")?.0,
        alternative_titles: r
            .try_get::<Json<Vec<AlternativeTitle>>, _>("alternative_titles")?
            .0,
        credits: r.try_get::<Json<Vec<Credit>>, _>("credits")?.0,
        popularity: r.try_get("popularity")?,
    })
}

fn movie_from_row(r: &SqliteRow) -> Result<MovieRecord> {
    let metadata = metadata_from_row(r)?;
    let movie = LibraryMovie {
        id: r.try_get("movie_id")?,
        tmdb_id: metadata.tmdb_id,
        title: metadata.title.clone(),
        year: metadata.year,
        monitored: r.try_get("monitored")?,
        minimum_availability: status(r.try_get("minimum_availability")?),
        quality_profile_id: r.try_get("quality_profile_id")?,
        path: r.try_get("path")?,
        tags: r.try_get::<Json<Vec<i32>>, _>("tags")?.0,
    };
    Ok(MovieRecord {
        movie,
        metadata_id: r.try_get("id")?,
        root_folder_path: r.try_get("root_folder_path")?,
        added: r.try_get("added")?,
        last_search_time: r.try_get("last_search_time")?,
        metadata,
    })
}

impl Movies<'_> {
    /// Inserts provider data, or refreshes it when the TMDB id is known.
    /// Returns the metadata row id.
    pub async fn upsert_metadata(&self, m: &MovieMetadata) -> Result<i64> {
        sqlx::query(
            "INSERT INTO movie_metadata
             (tmdb_id, imdb_id, title, clean_title, sort_title, original_title, original_language,
              overview, year, runtime, status, certification, genres, in_cinemas, physical_release,
              digital_release, collection_tmdb_id, collection_title, website, studio, rating_votes,
              rating_value, images, alternative_titles, credits, popularity, last_info_sync)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(tmdb_id) DO UPDATE SET
               imdb_id = excluded.imdb_id, title = excluded.title, clean_title = excluded.clean_title,
               sort_title = excluded.sort_title, original_title = excluded.original_title,
               original_language = excluded.original_language, overview = excluded.overview,
               year = excluded.year, runtime = excluded.runtime, status = excluded.status,
               certification = excluded.certification, genres = excluded.genres,
               in_cinemas = excluded.in_cinemas, physical_release = excluded.physical_release,
               digital_release = excluded.digital_release,
               collection_tmdb_id = excluded.collection_tmdb_id,
               collection_title = excluded.collection_title, website = excluded.website,
               studio = excluded.studio, rating_votes = excluded.rating_votes,
               rating_value = excluded.rating_value, images = excluded.images,
               alternative_titles = excluded.alternative_titles, credits = excluded.credits,
               popularity = excluded.popularity, last_info_sync = excluded.last_info_sync",
        )
        .bind(m.tmdb_id)
        .bind(&m.imdb_id)
        .bind(&m.title)
        .bind(clean_movie_title(&m.title))
        .bind(normalize_title(&m.title))
        .bind(&m.original_title)
        .bind(&m.original_language)
        .bind(&m.overview)
        .bind(m.year)
        .bind(i64::from(m.runtime))
        .bind(m.status as i32)
        .bind(&m.certification)
        .bind(Json(&m.genres))
        .bind(m.in_cinemas)
        .bind(m.physical_release)
        .bind(m.digital_release)
        .bind(m.collection_tmdb_id)
        .bind(&m.collection_title)
        .bind(&m.website)
        .bind(&m.studio)
        .bind(m.ratings.as_ref().map(|r| i64::from(r.votes)))
        .bind(m.ratings.as_ref().map(|r| r.value))
        .bind(Json(&m.images))
        .bind(Json(&m.alternative_titles))
        .bind(Json(&m.credits))
        .bind(m.popularity)
        .bind(Utc::now())
        .execute(self.0)
        .await?;
        Ok(
            sqlx::query_scalar("SELECT id FROM movie_metadata WHERE tmdb_id = ?")
                .bind(m.tmdb_id)
                .fetch_one(self.0)
                .await?,
        )
    }

    /// Adds a movie. A TMDB id can be in the library once: adding it again is
    /// [`DbError::Conflict`].
    pub async fn add(&self, n: &NewMovie) -> Result<i64> {
        let id = sqlx::query(
            "INSERT INTO movies (movie_metadata_id, monitored, minimum_availability,
             quality_profile_id, path, root_folder_path, added, tags)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(n.metadata_id)
        .bind(n.monitored)
        .bind(n.minimum_availability as i32)
        .bind(n.quality_profile_id)
        .bind(&n.path)
        .bind(&n.root_folder_path)
        .bind(Utc::now())
        .bind(Json(&n.tags))
        .execute(self.0)
        .await?
        .last_insert_rowid();
        Ok(id)
    }

    pub async fn get(&self, id: i64) -> Result<MovieRecord> {
        let row = sqlx::query(&format!("{SELECT_MOVIE} WHERE m.id = ?"))
            .bind(id)
            .fetch_optional(self.0)
            .await?
            .ok_or(DbError::NotFound)?;
        movie_from_row(&row)
    }

    pub async fn find_by_tmdb_id(&self, tmdb_id: i64) -> Result<Option<MovieRecord>> {
        sqlx::query(&format!("{SELECT_MOVIE} WHERE md.tmdb_id = ?"))
            .bind(tmdb_id)
            .fetch_optional(self.0)
            .await?
            .as_ref()
            .map(movie_from_row)
            .transpose()
    }

    pub async fn list(&self) -> Result<Vec<MovieRecord>> {
        sqlx::query(&format!("{SELECT_MOVIE} ORDER BY md.sort_title"))
            .fetch_all(self.0)
            .await?
            .iter()
            .map(movie_from_row)
            .collect()
    }

    /// Library movies whose cleaned title equals `clean_title`; the first step
    /// of matching a parsed release to the library.
    pub async fn find_by_clean_title(&self, clean_title: &str) -> Result<Vec<MovieRecord>> {
        sqlx::query(&format!("{SELECT_MOVIE} WHERE md.clean_title = ?"))
            .bind(clean_title)
            .fetch_all(self.0)
            .await?
            .iter()
            .map(movie_from_row)
            .collect()
    }

    pub async fn update(&self, id: i64, u: &MovieUpdate) -> Result<()> {
        let done = sqlx::query(
            "UPDATE movies SET monitored = ?, minimum_availability = ?, quality_profile_id = ?,
             path = ?, root_folder_path = ?, tags = ? WHERE id = ?",
        )
        .bind(u.monitored)
        .bind(u.minimum_availability as i32)
        .bind(u.quality_profile_id)
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

    pub async fn set_last_search_time(&self, id: i64, when: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE movies SET last_search_time = ? WHERE id = ?")
            .bind(when)
            .bind(id)
            .execute(self.0)
            .await?;
        Ok(())
    }

    /// Removes the movie and, through cascades, its file, history, blocklist
    /// entries and pending releases. Provider metadata stays until
    /// [`Self::purge_unused_metadata`].
    pub async fn delete(&self, id: i64) -> Result<()> {
        let done = sqlx::query("DELETE FROM movies WHERE id = ?")
            .bind(id)
            .execute(self.0)
            .await?;
        if done.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }

    /// Deletes metadata no movie refers to. Returns how many rows went.
    pub async fn purge_unused_metadata(&self) -> Result<u64> {
        Ok(sqlx::query(
            "DELETE FROM movie_metadata WHERE id NOT IN (SELECT movie_metadata_id FROM movies)",
        )
        .execute(self.0)
        .await?
        .rows_affected())
    }
}

// ---- movie files -------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct MovieFileRecord {
    pub id: i64,
    pub movie_id: i64,
    pub relative_path: String,
    pub size: u64,
    pub date_added: DateTime<Utc>,
    pub scene_name: Option<String>,
    pub release_group: Option<String>,
    pub edition: Option<String>,
    pub quality: QualityModel,
    pub languages: Vec<Language>,
    pub media_info: Option<serde_json::Value>,
    pub original_file_path: Option<String>,
    pub indexer_flags: IndexerFlags,
}

impl MovieFileRecord {
    /// The view the decision engine needs. Custom formats are matched by the
    /// caller, because that needs the movie and the current format list.
    pub fn to_existing_file(&self, custom_format_ids: Vec<i32>) -> ExistingFile {
        ExistingFile {
            quality: self.quality,
            size: self.size,
            release_group: self.release_group.clone(),
            date_added: self.date_added,
            custom_format_ids,
        }
    }
}

pub struct MovieFiles<'a>(pub(crate) &'a SqlitePool);

fn file_from_row(r: &SqliteRow) -> Result<MovieFileRecord> {
    Ok(MovieFileRecord {
        id: r.try_get("id")?,
        movie_id: r.try_get("movie_id")?,
        relative_path: r.try_get("relative_path")?,
        size: r.try_get::<i64, _>("size")? as u64,
        date_added: r.try_get("date_added")?,
        scene_name: r.try_get("scene_name")?,
        release_group: r.try_get("release_group")?,
        edition: r.try_get("edition")?,
        quality: r.try_get::<Json<QualityModel>, _>("quality")?.0,
        languages: r.try_get::<Json<Vec<Language>>, _>("languages")?.0,
        media_info: r
            .try_get::<Option<Json<serde_json::Value>>, _>("media_info")?
            .map(|j| j.0),
        original_file_path: r.try_get("original_file_path")?,
        indexer_flags: IndexerFlags(r.try_get::<i64, _>("indexer_flags")? as u32),
    })
}

impl MovieFiles<'_> {
    /// Sets the file for a movie, replacing any existing one (a movie has at
    /// most one file). The record's `id` is ignored; the new id is returned.
    pub async fn set(&self, f: &MovieFileRecord) -> Result<i64> {
        sqlx::query("DELETE FROM movie_files WHERE movie_id = ?")
            .bind(f.movie_id)
            .execute(self.0)
            .await?;
        let id = sqlx::query(
            "INSERT INTO movie_files (movie_id, relative_path, size, date_added, scene_name,
             release_group, edition, quality, languages, media_info, original_file_path, indexer_flags)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(f.movie_id)
        .bind(&f.relative_path)
        .bind(f.size as i64)
        .bind(f.date_added)
        .bind(&f.scene_name)
        .bind(&f.release_group)
        .bind(&f.edition)
        .bind(Json(&f.quality))
        .bind(Json(&f.languages))
        .bind(f.media_info.as_ref().map(Json))
        .bind(&f.original_file_path)
        .bind(i64::from(f.indexer_flags.bits()))
        .execute(self.0)
        .await?
        .last_insert_rowid();
        Ok(id)
    }

    pub async fn for_movie(&self, movie_id: i64) -> Result<Option<MovieFileRecord>> {
        sqlx::query("SELECT * FROM movie_files WHERE movie_id = ?")
            .bind(movie_id)
            .fetch_optional(self.0)
            .await?
            .as_ref()
            .map(file_from_row)
            .transpose()
    }

    pub async fn delete_for_movie(&self, movie_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM movie_files WHERE movie_id = ?")
            .bind(movie_id)
            .execute(self.0)
            .await?;
        Ok(())
    }
}
