//! Keeping the TV library and the disk in step: rescanning a series' folder
//! and renaming files to match the naming format. Modelled on Sonarr's
//! `DiskScanService` and `RenameEpisodeFileService` (GPL-3.0).

use std::path::Path;

use chrono::{DateTime, Utc};
use omp_core::{
    history::HistoryEvent,
    quality::{DetectionSource, Quality, QualityModel},
};
use omp_db::{
    Db,
    repo::{EpisodeFileRecord, EpisodeHistoryRecord, EpisodeRecord, SeriesRecord},
};
use omp_naming::{NamingConfig, NamingFile, build_episode_file_name, build_season_folder};
use omp_parser::{files::quality_for_extension, tv::parse_path};

use crate::{
    evaluate::ServiceError,
    formats::matching_ids,
    scan::{extension_of, media_files},
    tv_evaluate::input_for_episode_file,
    tv_import::{naming_episode, naming_series},
    tv_match::pick_episodes,
};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SeriesRescan {
    /// Files on disk that were recorded.
    pub added: usize,
    /// Recorded files that are gone from disk and were forgotten.
    pub removed: usize,
    /// Files on disk that could not be matched to any episode.
    pub unmatched: Vec<String>,
}

fn file_event(
    series: &SeriesRecord,
    e: &EpisodeRecord,
    event: HistoryEvent,
    now: DateTime<Utc>,
    f: &EpisodeFileRecord,
    data: Vec<(&str, String)>,
) -> EpisodeHistoryRecord {
    EpisodeHistoryRecord {
        id: 0,
        series_id: series.id,
        episode_id: e.id,
        event,
        date: now,
        source_title: f.relative_path.clone(),
        quality: f.quality,
        languages: f.languages.clone(),
        download_id: None,
        data: data.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
    }
}

/// Brings the database in line with the series' folder: records files that
/// are there and match an episode, forgets those that are gone.
pub async fn rescan_series(
    db: &Db,
    series_id: i64,
    now: DateTime<Utc>,
) -> Result<SeriesRescan, ServiceError> {
    let series = db.series().get(series_id).await?;
    let root = Path::new(&series.path);
    let mut out = SeriesRescan::default();
    let episodes = db.episodes().for_series(series_id).await?;

    // Forget what is no longer on disk.
    for f in db.episode_files().for_series(series_id).await? {
        if root.join(&f.relative_path).is_file() {
            continue;
        }
        for e in episodes.iter().filter(|e| e.episode_file_id == Some(f.id)) {
            db.episode_history()
                .insert(&file_event(
                    &series,
                    e,
                    HistoryEvent::MovieFileDeleted,
                    now,
                    &f,
                    vec![("reason", "missing from disk".into())],
                ))
                .await?;
        }
        db.episode_files().delete(f.id).await?;
        out.removed += 1;
    }

    // Record new files.
    let episodes = db.episodes().for_series(series_id).await?;
    let known: Vec<String> = db
        .episode_files()
        .for_series(series_id)
        .await?
        .into_iter()
        .map(|f| f.relative_path)
        .collect();
    let profile = db.quality_profiles().get(series.quality_profile_id).await?;
    let mut found = Vec::new();
    for path in media_files(root)
        .await
        .map_err(|e| ServiceError::Io(e.to_string()))?
    {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        if known.contains(&relative) {
            continue;
        }
        let Some(parsed) = parse_path(&relative).filter(|p| !p.full_season) else {
            out.unmatched.push(relative);
            continue;
        };
        let season = parsed
            .season_number()
            .or_else(|| parsed.is_daily().then_some(0))
            .unwrap_or(0);
        let matched = pick_episodes(&parsed, &episodes, season);
        if matched.is_empty() {
            out.unmatched.push(relative);
            continue;
        }
        found.push((path, relative, parsed, matched));
    }
    // Several files for one episode: keep the best.
    found.sort_by(|a, b| profile.comparer().compare_model(&b.2.quality, &a.2.quality));
    let mut taken: Vec<i64> = episodes
        .iter()
        .filter(|e| e.has_file())
        .map(|e| e.id)
        .collect();
    for (path, relative, parsed, matched) in found {
        if matched.iter().any(|e| taken.contains(&e.id)) {
            out.unmatched.push(relative);
            continue;
        }
        let meta = tokio::fs::metadata(&path)
            .await
            .map_err(|e| ServiceError::Io(e.to_string()))?;
        let mut quality = parsed.quality;
        if quality.quality == Quality::UNKNOWN {
            quality = QualityModel {
                quality: quality_for_extension(&extension_of(&path)),
                source_detection: DetectionSource::Extension,
                resolution_detection: DetectionSource::Extension,
                ..quality
            };
        }
        let first = &matched[0];
        let id = db
            .episode_files()
            .insert(&EpisodeFileRecord {
                id: 0,
                series_id,
                season_number: first.season_number,
                relative_path: relative,
                size: meta.len(),
                date_added: meta
                    .modified()
                    .map(DateTime::<Utc>::from)
                    .unwrap_or(now)
                    .min(now),
                scene_name: None,
                release_group: parsed.release_group.clone(),
                quality,
                languages: parsed.languages.clone(),
                media_info: None,
                original_file_path: None,
                indexer_flags: omp_core::release::IndexerFlags(0),
            })
            .await?;
        let ids: Vec<i64> = matched.iter().map(|e| e.id).collect();
        db.episodes().set_file(&ids, Some(id)).await?;
        taken.extend(ids);
        out.added += 1;
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeRenamePreview {
    pub series_id: i64,
    pub series_title: String,
    pub episode_file_id: i64,
    pub episode_ids: Vec<i64>,
    /// Relative to the series folder.
    pub existing: String,
    pub new: String,
}

async fn previews_for(
    db: &Db,
    naming: &NamingConfig,
    series: &SeriesRecord,
) -> Result<Vec<EpisodeRenamePreview>, ServiceError> {
    if !naming.rename_episodes {
        return Ok(vec![]);
    }
    let formats = db.custom_formats().load_compiled().await?;
    let episodes = db.episodes().for_series(series.id).await?;
    let mut out = Vec::new();
    for f in db.episode_files().for_series(series.id).await? {
        let mut mine: Vec<&EpisodeRecord> = episodes
            .iter()
            .filter(|e| e.episode_file_id == Some(f.id))
            .collect();
        if mine.is_empty() {
            continue;
        }
        mine.sort_by_key(|e| (e.season_number, e.episode_number));
        let ids = matching_ids(&formats, &input_for_episode_file(&f, series));
        let naming_file = NamingFile {
            quality_title: Some(f.quality.quality.name.to_owned()),
            revision_version: f.quality.revision.version as i32,
            revision_real: f.quality.revision.real as i32,
            release_group: f.release_group.clone(),
            scene_name: f.scene_name.clone().filter(|s| !s.is_empty()),
            relative_path: Some(f.relative_path.clone()),
            path: Some(
                Path::new(&series.path)
                    .join(&f.relative_path)
                    .to_string_lossy()
                    .into_owned(),
            ),
            edition: None,
            media: f
                .media_info
                .as_ref()
                .and_then(|raw| crate::media::naming_media(raw, f.scene_name.as_deref())),
            custom_formats: formats
                .iter()
                .filter(|c| c.include_when_renaming && ids.contains(&c.id))
                .map(|c| c.name.clone())
                .collect(),
            release_hash: None,
        };
        let eps: Vec<_> = mine.iter().map(|e| naming_episode(e)).collect();
        let extension = extension_of(Path::new(&f.relative_path));
        let name = build_episode_file_name(
            &naming_series(series),
            &eps,
            &naming_file,
            extension.len(),
            naming,
        )
        .map_err(|e| ServiceError::Io(format!("naming failed: {e}")))?;
        let mut relative = String::new();
        if series.season_folder {
            let title = series
                .season(mine[0].season_number)
                .and_then(|s| s.title.clone());
            relative = build_season_folder(
                &naming_series(series),
                mine[0].season_number,
                title.as_deref(),
                naming,
            );
            relative.push('/');
        }
        relative.push_str(&name);
        relative.push_str(&extension);
        if relative != f.relative_path {
            out.push(EpisodeRenamePreview {
                series_id: series.id,
                series_title: series.info.title.clone(),
                episode_file_id: f.id,
                episode_ids: mine.iter().map(|e| e.id).collect(),
                existing: f.relative_path.clone(),
                new: relative,
            });
        }
    }
    Ok(out)
}

/// What renaming would change for one series, or for the whole library.
pub async fn episode_rename_previews(
    db: &Db,
    naming: &NamingConfig,
    only: Option<i64>,
) -> Result<Vec<EpisodeRenamePreview>, ServiceError> {
    let mut out = Vec::new();
    match only {
        Some(id) => out.extend(previews_for(db, naming, &db.series().get(id).await?).await?),
        None => {
            for s in db.series().list().await? {
                out.extend(previews_for(db, naming, &s).await?);
            }
        }
    }
    Ok(out)
}

/// Renames the series' files on disk and in the database. Refuses to
/// overwrite anything. Returns how many were renamed.
pub async fn rename_series(
    db: &Db,
    naming: &NamingConfig,
    series_id: i64,
    now: DateTime<Utc>,
) -> Result<usize, ServiceError> {
    let series = db.series().get(series_id).await?;
    let files = db.episode_files().for_series(series_id).await?;
    let episodes = db.episodes().for_series(series_id).await?;
    let mut renamed = 0;
    for p in previews_for(db, naming, &series).await? {
        let folder = Path::new(&series.path);
        let (from, to) = (folder.join(&p.existing), folder.join(&p.new));
        if to.exists() {
            return Err(ServiceError::Io(format!("{} already exists", to.display())));
        }
        if let Some(parent) = to.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                ServiceError::Io(format!("cannot create {}: {e}", parent.display()))
            })?;
        }
        tokio::fs::rename(&from, &to)
            .await
            .map_err(|e| ServiceError::Io(format!("cannot rename {}: {e}", from.display())))?;
        db.episode_files()
            .update_path(p.episode_file_id, &p.new)
            .await?;
        if let Some(f) = files.iter().find(|f| f.id == p.episode_file_id) {
            for e in episodes.iter().filter(|e| p.episode_ids.contains(&e.id)) {
                db.episode_history()
                    .insert(&file_event(
                        &series,
                        e,
                        HistoryEvent::MovieFileRenamed,
                        now,
                        f,
                        vec![("source_path", p.existing.clone()), ("path", p.new.clone())],
                    ))
                    .await?;
            }
        }
        renamed += 1;
    }
    Ok(renamed)
}
