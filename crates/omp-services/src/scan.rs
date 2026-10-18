//! Finding what is already on disk: rescanning a movie's folder for its file,
//! and listing folders in a root folder that are not in the library yet.
//! Modelled on Radarr's `DiskScanService` and library import (GPL-3.0).

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use omp_core::{
    history::HistoryEvent,
    quality::{DetectionSource, Quality, QualityModel},
};
use omp_db::{
    Db,
    repo::{HistoryRecord, MovieFileRecord},
};
use omp_parser::{
    files::{is_media_extension, quality_for_extension},
    title::parse_movie_title,
};

use crate::evaluate::ServiceError;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct RescanResult {
    /// A file on disk was recorded for the movie.
    pub added: bool,
    /// The recorded file is gone from disk and the record was removed.
    pub removed: bool,
    /// Something is recorded for the movie after the scan.
    pub has_file: bool,
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default()
}

/// Every media file under `root`, deepest folders included.
async fn media_files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(e) => e,
            // The movie's own folder may not exist yet; a sub folder may vanish mid-scan.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let kind = entry.file_type().await?;
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() && is_media_extension(&extension_of(&path)) {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

struct Found {
    path: PathBuf,
    size: u64,
    modified: DateTime<Utc>,
    quality: QualityModel,
    release_group: Option<String>,
    edition: Option<String>,
    languages: Vec<omp_core::language::Language>,
}

async fn describe(path: PathBuf) -> std::io::Result<Found> {
    let meta = tokio::fs::metadata(&path).await?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parsed = parse_movie_title(&name, false);
    let mut quality = parsed.as_ref().map(|p| p.quality).unwrap_or_default();
    if quality.quality == Quality::UNKNOWN {
        quality = QualityModel {
            quality: quality_for_extension(&extension_of(&path)),
            source_detection: DetectionSource::Extension,
            resolution_detection: DetectionSource::Extension,
            ..quality
        };
    }
    Ok(Found {
        size: meta.len(),
        modified: meta
            .modified()
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(|_| Utc::now()),
        release_group: parsed.as_ref().and_then(|p| p.release_group.clone()),
        edition: parsed
            .as_ref()
            .map(|p| p.edition.clone())
            .filter(|e| !e.is_empty()),
        languages: parsed
            .as_ref()
            .map(|p| p.languages.clone())
            .unwrap_or_default(),
        quality,
        path,
    })
}

/// Brings the database in line with the movie's folder: records a file that
/// is there, forgets one that is gone. The best file wins when several are
/// present (highest quality in the movie's profile, then largest).
pub async fn rescan_movie(
    db: &Db,
    movie_id: i64,
    now: DateTime<Utc>,
) -> Result<RescanResult, ServiceError> {
    let movie = db.movies().get(movie_id).await?;
    let profile = db
        .quality_profiles()
        .get(movie.movie.quality_profile_id)
        .await?;
    let root = Path::new(&movie.movie.path);
    let existing = db.movie_files().for_movie(movie_id).await?;
    let mut result = RescanResult::default();

    // Is the recorded file still there?
    let recorded_present = existing
        .as_ref()
        .is_some_and(|f| root.join(&f.relative_path).is_file());
    if recorded_present {
        result.has_file = true;
        return Ok(result);
    }

    let mut files = Vec::new();
    for p in media_files(root)
        .await
        .map_err(|e| ServiceError::Io(e.to_string()))?
    {
        if let Ok(f) = describe(p).await {
            files.push(f);
        }
    }
    files.sort_by(|a, b| {
        profile
            .comparer()
            .compare_model(&b.quality, &a.quality)
            .then(b.size.cmp(&a.size))
    });

    match (files.into_iter().next(), existing) {
        (Some(best), old) => {
            let relative = best
                .path
                .strip_prefix(root)
                .unwrap_or(&best.path)
                .to_string_lossy()
                .into_owned();
            db.movie_files()
                .set(&MovieFileRecord {
                    id: 0,
                    movie_id,
                    relative_path: relative,
                    size: best.size,
                    date_added: best.modified.min(now),
                    scene_name: None,
                    release_group: best.release_group,
                    edition: best.edition,
                    quality: best.quality,
                    languages: best.languages,
                    media_info: None,
                    original_file_path: None,
                    indexer_flags: omp_core::release::IndexerFlags(0),
                })
                .await?;
            let _ = old;
            result.added = true;
            result.has_file = true;
        }
        (None, Some(old)) => {
            db.movie_files().delete_for_movie(movie_id).await?;
            db.history()
                .insert(&HistoryRecord {
                    id: 0,
                    movie_id,
                    event: HistoryEvent::MovieFileDeleted,
                    date: now,
                    source_title: old.relative_path.clone(),
                    quality: old.quality,
                    languages: old.languages.clone(),
                    download_id: None,
                    data: [("reason".to_owned(), "missing from disk".to_owned())].into(),
                })
                .await?;
            result.removed = true;
        }
        (None, None) => {}
    }
    Ok(result)
}

/// A folder inside a root folder that no movie in the library uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmappedFolder {
    pub name: String,
    pub path: PathBuf,
    /// What the folder name says the movie is.
    pub title: String,
    pub year: Option<i32>,
    pub has_media: bool,
}

/// Lists the sub folders of `root` that are not a movie's folder yet, with
/// the title and year their names suggest. Folders without any media file are
/// included but flagged, so the person can skip them.
pub async fn unmapped_folders(db: &Db, root: &Path) -> Result<Vec<UnmappedFolder>, ServiceError> {
    let known: std::collections::HashSet<PathBuf> = db
        .movies()
        .list()
        .await?
        .into_iter()
        .map(|m| PathBuf::from(m.movie.path))
        .collect();

    let mut out = Vec::new();
    let mut entries = tokio::fs::read_dir(root)
        .await
        .map_err(|e| ServiceError::Io(e.to_string()))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| ServiceError::Io(e.to_string()))?
    {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
        if !is_dir || name.starts_with('.') || known.contains(&path) {
            continue;
        }
        let parsed = parse_movie_title(&name, true);
        let (title, year) = match &parsed {
            Some(p) => (
                p.primary_title().unwrap_or(&name).to_owned(),
                (p.year != 0).then_some(p.year),
            ),
            None => (name.clone(), None),
        };
        let has_media = !media_files(&path).await.unwrap_or_default().is_empty();
        out.push(UnmappedFolder {
            name,
            path,
            title,
            year,
            has_media,
        });
    }
    out.sort_by_key(|f| f.name.to_lowercase());
    Ok(out)
}
