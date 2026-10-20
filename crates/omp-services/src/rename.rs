//! Renaming files already in the library to match the current naming format
//! (Radarr's "Organize"/"Rename files").

use std::path::Path;

use chrono::{DateTime, Utc};
use omp_core::history::HistoryEvent;
use omp_db::{
    Db,
    repo::{HistoryRecord, MovieFileRecord, MovieRecord},
};
use omp_naming::{NamingConfig, NamingFile, NamingMovie, build_file_name};

use crate::{
    evaluate::ServiceError,
    formats::{input_for_file, matching_ids},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamePreview {
    pub movie_id: i64,
    pub title: String,
    /// Relative to the movie's folder.
    pub existing: String,
    pub new: String,
}

fn extension_of(relative: &str) -> String {
    Path::new(relative)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default()
}

/// The name this file would get now, or `None` when it already has it.
async fn target_name(
    db: &Db,
    naming: &NamingConfig,
    movie: &MovieRecord,
    file: &MovieFileRecord,
) -> Result<Option<String>, ServiceError> {
    let formats = db.custom_formats().load_compiled().await?;
    let ids = matching_ids(&formats, &input_for_file(file, movie));
    let custom_formats = formats
        .iter()
        .filter(|f| f.include_when_renaming && ids.contains(&f.id))
        .map(|f| f.name.clone())
        .collect();

    let naming_movie = NamingMovie {
        title: movie.movie.title.clone(),
        original_title: Some(movie.metadata.original_title.clone()).filter(|t| !t.is_empty()),
        year: movie.movie.year,
        imdb_id: movie.metadata.imdb_id.clone(),
        tmdb_id: movie.movie.tmdb_id,
        certification: movie.metadata.certification.clone(),
        collection_title: movie.metadata.collection_title.clone(),
        translations: Vec::new(),
    };
    let naming_file = NamingFile {
        quality_title: Some(file.quality.quality.name.to_owned()),
        revision_version: file.quality.revision.version as i32,
        revision_real: file.quality.revision.real as i32,
        release_group: file.release_group.clone(),
        scene_name: file.scene_name.clone().filter(|s| !s.is_empty()),
        relative_path: Some(file.relative_path.clone()),
        path: Some(
            Path::new(&movie.movie.path)
                .join(&file.relative_path)
                .to_string_lossy()
                .into_owned(),
        ),
        edition: file.edition.clone().filter(|e| !e.is_empty()),
        media: None,
        custom_formats,
    };
    let name = build_file_name(&naming_movie, &naming_file, naming)
        .map_err(|e| ServiceError::Io(format!("naming failed: {e}")))?;
    let relative = format!("{name}{}", extension_of(&file.relative_path));
    Ok((relative != file.relative_path).then_some(relative))
}

/// What renaming this movie's file would change. Empty when the name is
/// already right, or there is no file. Renaming is off when the setting says
/// not to rename movies.
pub async fn rename_preview(
    db: &Db,
    naming: &NamingConfig,
    movie_id: i64,
) -> Result<Option<RenamePreview>, ServiceError> {
    if !naming.rename_movies {
        return Ok(None);
    }
    let movie = db.movies().get(movie_id).await?;
    let Some(file) = db.movie_files().for_movie(movie_id).await? else {
        return Ok(None);
    };
    Ok(target_name(db, naming, &movie, &file)
        .await?
        .map(|new| RenamePreview {
            movie_id,
            title: movie.movie.title.clone(),
            existing: file.relative_path,
            new,
        }))
}

/// Every library movie whose file would be renamed.
pub async fn rename_previews(
    db: &Db,
    naming: &NamingConfig,
) -> Result<Vec<RenamePreview>, ServiceError> {
    let mut out = Vec::new();
    for m in db.movies().list().await? {
        if let Some(p) = rename_preview(db, naming, m.movie.id).await? {
            out.push(p);
        }
    }
    Ok(out)
}

/// Renames the movie's file on disk and in the database. Refuses to
/// overwrite anything.
pub async fn rename_movie(
    db: &Db,
    naming: &NamingConfig,
    movie_id: i64,
    now: DateTime<Utc>,
) -> Result<Option<RenamePreview>, ServiceError> {
    let Some(preview) = rename_preview(db, naming, movie_id).await? else {
        return Ok(None);
    };
    let movie = db.movies().get(movie_id).await?;
    let Some(file) = db.movie_files().for_movie(movie_id).await? else {
        return Ok(None);
    };
    let folder = Path::new(&movie.movie.path);
    let from = folder.join(&preview.existing);
    let to = folder.join(&preview.new);
    if to.exists() {
        return Err(ServiceError::Io(format!("{} already exists", to.display())));
    }
    if let Some(parent) = to.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| ServiceError::Io(format!("cannot create {}: {e}", parent.display())))?;
    }
    tokio::fs::rename(&from, &to)
        .await
        .map_err(|e| ServiceError::Io(format!("cannot rename {}: {e}", from.display())))?;
    db.movie_files()
        .set(&MovieFileRecord {
            relative_path: preview.new.clone(),
            ..file.clone()
        })
        .await?;
    db.history()
        .insert(&HistoryRecord {
            id: 0,
            movie_id,
            event: HistoryEvent::MovieFileRenamed,
            date: now,
            source_title: preview.existing.clone(),
            quality: file.quality,
            languages: file.languages.clone(),
            download_id: None,
            data: [
                ("source_path".to_owned(), preview.existing.clone()),
                ("path".to_owned(), preview.new.clone()),
            ]
            .into(),
        })
        .await?;
    Ok(Some(preview))
}
