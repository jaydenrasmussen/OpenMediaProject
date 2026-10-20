//! Importing a finished TV download into the library. Ported from Sonarr
//! `DownloadedEpisodesImportService`, `ImportDecisionMaker` and
//! `EpisodeImportService` (GPL-3.0).

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use omp_core::{history::HistoryEvent, quality::Quality, tv::SeriesType};
use omp_db::{
    Db,
    repo::{EpisodeFileRecord, EpisodeHistoryRecord, EpisodeRecord, SeriesRecord},
};
use omp_naming::{
    NamingEpisode, NamingFile, NamingSeries, NamingSeriesType, build_episode_file_name,
    build_season_folder,
};
use omp_parser::tv::{ParsedEpisodeInfo, parse_title};
use omp_quality::{
    custom_format::{CustomFormat, CustomFormatInput},
    upgrade::{Candidate, ProperDownloadTypes, UpgradeRejectReason, is_upgradable},
};

use crate::{
    DiskSpace,
    evaluate::ServiceError,
    formats::matching_ids,
    import::{
        FileOutcome, FileResult, ImportConfig, ImportMode, ImportRejection, ImportResult,
        MediaProbe, ProbeInfo, extension_of, find_media_files, in_working_folder,
        minimum_runtime_seconds, transfer,
    },
    tv_match::pick_episodes,
};

pub struct TvImportRequest<'a> {
    pub series_id: i64,
    pub output_path: &'a Path,
    pub download_id: Option<&'a str>,
    pub download_title: &'a str,
    /// The episodes the grab was for; empty for a manual import.
    pub episode_ids: &'a [i64],
    pub can_move_files: bool,
    pub now: DateTime<Utc>,
    pub manual: Option<crate::import::ManualImport>,
}

pub struct TvImporter<'a> {
    pub db: &'a Db,
    pub config: &'a ImportConfig,
    pub probe: &'a dyn MediaProbe,
    pub disk: &'a dyn DiskSpace,
}

struct Candidate_ {
    path: PathBuf,
    size: u64,
    quality: omp_core::quality::QualityModel,
    release_group: Option<String>,
    scene_name: String,
    languages: Vec<omp_core::language::Language>,
    probe: Option<ProbeInfo>,
    episodes: Vec<EpisodeRecord>,
    release_hash: Option<String>,
}

fn series_type(t: SeriesType) -> NamingSeriesType {
    match t {
        SeriesType::Standard => NamingSeriesType::Standard,
        SeriesType::Daily => NamingSeriesType::Daily,
        SeriesType::Anime => NamingSeriesType::Anime,
    }
}

pub fn naming_series(s: &SeriesRecord) -> NamingSeries {
    NamingSeries {
        title: s.info.title.clone(),
        year: s.info.year,
        imdb_id: s.info.imdb_id.clone(),
        tvdb_id: s.info.tvdb_id,
        tmdb_id: s.info.tmdb_id,
        series_type: series_type(s.series_type),
    }
}

pub fn naming_episode(e: &EpisodeRecord) -> NamingEpisode {
    NamingEpisode {
        season: e.season_number,
        episode: e.episode_number,
        absolute: e.absolute_episode_number,
        title: e.title.clone(),
        air_date: e.air_date.map(|d| d.to_string()),
    }
}

fn format_input(c: &Candidate_, series: &SeriesRecord) -> CustomFormatInput {
    CustomFormatInput {
        quality: Some(c.quality.quality),
        edition: None,
        release_group: c.release_group.clone(),
        simple_release_title: Some(c.scene_name.clone()).filter(|s| !s.is_empty()),
        filename: c.path.file_name().map(|n| n.to_string_lossy().into_owned()),
        parsed_year: 0,
        movie_year: Some(series.info.year),
        movie_original_language: omp_core::language::Language::from_iso639_1(
            &series.info.original_language,
        ),
        size: c.size,
        indexer_flags: omp_core::release::IndexerFlags(0),
        languages: c.languages.clone(),
    }
}

impl TvImporter<'_> {
    pub async fn import(&self, req: &TvImportRequest<'_>) -> Result<ImportResult, ServiceError> {
        let series = self.db.series().get(req.series_id).await?;
        let profile = self
            .db
            .quality_profiles()
            .get(series.quality_profile_id)
            .await?;
        let formats = self.db.custom_formats().load_compiled().await?;
        let all_episodes = self.db.episodes().for_series(series.id).await?;
        let existing_files = self.db.episode_files().for_series(series.id).await?;
        let history = match req.download_id {
            Some(id) => self.db.episode_history().by_download_id(id).await?,
            None => Vec::new(),
        };

        let paths = match find_media_files(req.output_path).await {
            Ok(p) => p,
            Err(e) => {
                return Ok(ImportResult {
                    files: vec![FileResult {
                        path: req.output_path.to_path_buf(),
                        outcome: FileOutcome::Failed(format!(
                            "Cannot read {}: {e}",
                            req.output_path.display()
                        )),
                    }],
                });
            }
        };
        let from_download = parse_title(req.download_title);
        let mut result = ImportResult::default();
        let mut approved: Vec<Candidate_> = Vec::new();
        let propers = if self.config.download_propers {
            ProperDownloadTypes::PreferAndUpgrade
        } else {
            ProperDownloadTypes::DoNotPrefer
        };

        for path in &paths {
            let mut rejections: Vec<ImportRejection> = Vec::new();
            let mut candidate = self
                .describe(
                    path,
                    req,
                    &all_episodes,
                    from_download.as_ref(),
                    paths.len(),
                )
                .await;
            if let Some(q) = req.manual.as_ref().and_then(|m| m.quality) {
                candidate.quality = q;
            }
            if candidate.episodes.is_empty() {
                result.files.push(FileResult {
                    path: path.clone(),
                    outcome: FileOutcome::Rejected(vec![ImportRejection::Other(
                        "Unable to identify the episode(s) in this file".into(),
                    )]),
                });
                continue;
            }
            if !req.episode_ids.is_empty()
                && candidate
                    .episodes
                    .iter()
                    .any(|e| !req.episode_ids.contains(&e.id))
            {
                rejections.push(ImportRejection::Other(
                    "Episode wasn't part of the grabbed release".into(),
                ));
            }
            if in_working_folder(path, &self.config.working_folders) {
                rejections.push(ImportRejection::Unpacking);
            }
            match &candidate.probe {
                None => rejections.push(ImportRejection::SampleIndeterminate),
                Some(p) => {
                    let ext = extension_of(path).to_lowercase();
                    let exempt =
                        matches!(ext.as_str(), ".flv" | ".strm" | ".iso" | ".img" | ".m2ts");
                    if !exempt
                        && !candidate.episodes.iter().any(|e| e.season_number == 0)
                        && p.runtime_seconds < minimum_runtime_seconds(series.info.runtime)
                    {
                        rejections.push(ImportRejection::Sample);
                    }
                    if p.audio_streams == 0 {
                        rejections.push(ImportRejection::NoAudio);
                    }
                }
            }
            if !self.config.skip_free_space_check {
                let dir = Path::new(&series.path).parent().unwrap_or(Path::new("/"));
                if let Some(free) = self.disk.free_bytes(&dir.to_string_lossy())
                    && free < candidate.size + self.config.minimum_free_space_mb * 1024 * 1024
                {
                    rejections.push(ImportRejection::NotEnoughFreeSpace);
                }
            }

            // Already imported by this very download.
            let ids: Vec<i64> = candidate.episodes.iter().map(|e| e.id).collect();
            if let Some(at) = history
                .iter()
                .filter(|h| {
                    h.event == HistoryEvent::DownloadFolderImported && ids.contains(&h.episode_id)
                })
                .map(|h| h.date)
                .max()
                && !history
                    .iter()
                    .any(|h| h.event == HistoryEvent::Grabbed && h.date > at)
            {
                rejections.push(ImportRejection::AlreadyImported(at));
            }

            // Every episode that already has a file must be improved on.
            let mut old_ids: Vec<i64> = candidate
                .episodes
                .iter()
                .filter_map(|e| e.episode_file_id)
                .collect();
            old_ids.sort_unstable();
            old_ids.dedup();
            let new_ids = matching_ids(&formats, &format_input(&candidate, &series));
            for old in old_ids
                .iter()
                .filter_map(|id| existing_files.iter().find(|f| f.id == *id))
            {
                let covered: Vec<i64> = all_episodes
                    .iter()
                    .filter(|e| e.episode_file_id == Some(old.id))
                    .map(|e| e.id)
                    .collect();
                if covered.iter().any(|id| !ids.contains(id)) {
                    rejections.push(ImportRejection::Other(
                        "Existing file has more episodes than the new file".into(),
                    ));
                    continue;
                }
                let old_input = crate::tv_evaluate::input_for_episode_file(old, &series);
                let old_formats = matching_ids(&formats, &old_input);
                let reason = is_upgradable(
                    &profile,
                    propers,
                    Candidate {
                        quality: &old.quality,
                        format_ids: &old_formats,
                    },
                    Candidate {
                        quality: &candidate.quality,
                        format_ids: &new_ids,
                    },
                );
                if reason != UpgradeRejectReason::None {
                    rejections.push(ImportRejection::NotQualityUpgrade(format!(
                        "Not an upgrade for existing episode file. Existing quality: {}. New quality: {}",
                        old.quality.quality.name, candidate.quality.quality.name
                    )));
                }
            }

            if req.manual.is_some() {
                rejections.clear();
            }
            if rejections.is_empty() {
                candidate.release_hash = from_download
                    .as_ref()
                    .map(|p| p.release_hash.clone())
                    .filter(|h| !h.is_empty());
                approved.push(candidate);
            } else {
                result.files.push(FileResult {
                    path: path.clone(),
                    outcome: FileOutcome::Rejected(rejections),
                });
            }
        }

        // Best first; an episode is imported once per run.
        approved.sort_by(|a, b| {
            profile
                .comparer()
                .compare_model(&b.quality, &a.quality)
                .then(b.episodes.len().cmp(&a.episodes.len()))
                .then(b.size.cmp(&a.size))
        });
        let mut taken: Vec<i64> = Vec::new();
        for candidate in approved {
            if candidate.episodes.iter().any(|e| taken.contains(&e.id)) {
                result.files.push(FileResult {
                    path: candidate.path,
                    outcome: FileOutcome::Rejected(vec![ImportRejection::AlreadyImportedThisRun]),
                });
                continue;
            }
            let outcome = match self.import_file(&series, &candidate, req, &formats).await {
                Ok(to) => {
                    taken.extend(candidate.episodes.iter().map(|e| e.id));
                    FileOutcome::Imported { to }
                }
                Err(message) => FileOutcome::Failed(message),
            };
            result.files.push(FileResult {
                path: candidate.path,
                outcome,
            });
        }
        Ok(result)
    }

    async fn describe(
        &self,
        path: &Path,
        req: &TvImportRequest<'_>,
        all_episodes: &[EpisodeRecord],
        from_download: Option<&ParsedEpisodeInfo>,
        file_count: usize,
    ) -> Candidate_ {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let from_file = parse_title(&file_name);

        let mut quality = from_file.as_ref().map(|p| p.quality).unwrap_or_default();
        if quality.quality == Quality::UNKNOWN
            && let Some(d) = from_download
        {
            quality = d.quality;
        }
        let release_group = from_file
            .as_ref()
            .and_then(|p| p.release_group.clone())
            .or_else(|| from_download.and_then(|p| p.release_group.clone()));
        let languages = from_file
            .as_ref()
            .or(from_download)
            .map(|p| p.languages.clone())
            .unwrap_or_default();

        // The file's own numbers win; a lone file in a one-episode grab takes the grab's episodes.
        let mut episodes = from_file
            .as_ref()
            .filter(|p| !p.full_season)
            .and_then(|p| p.season_number().map(|s| pick_episodes(p, all_episodes, s)))
            .unwrap_or_default();
        if episodes.is_empty() && file_count == 1 && !req.episode_ids.is_empty() {
            episodes = all_episodes
                .iter()
                .filter(|e| req.episode_ids.contains(&e.id))
                .cloned()
                .collect();
        }
        if episodes.is_empty()
            && let Some(d) = from_download.filter(|d| !d.full_season)
            && let Some(s) = d.season_number()
            && file_count == 1
        {
            episodes = pick_episodes(d, all_episodes, s);
        }
        // A person who picked the episodes by hand is right, whatever the name says.
        if req.manual.is_some() && !req.episode_ids.is_empty() {
            episodes = all_episodes
                .iter()
                .filter(|e| req.episode_ids.contains(&e.id))
                .cloned()
                .collect();
        }
        episodes.sort_by_key(|e| (e.season_number, e.episode_number));
        let size = tokio::fs::metadata(path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        Candidate_ {
            path: path.to_path_buf(),
            size,
            quality,
            release_group,
            scene_name: if from_download.is_some() {
                req.download_title.to_owned()
            } else {
                String::new()
            },
            languages,
            probe: self.probe.probe(path).await,
            episodes,
            release_hash: None,
        }
    }

    async fn import_file(
        &self,
        series: &SeriesRecord,
        c: &Candidate_,
        req: &TvImportRequest<'_>,
        formats: &[CustomFormat],
    ) -> Result<PathBuf, String> {
        let extension = extension_of(&c.path);
        let ids = matching_ids(formats, &format_input(c, series));
        let custom_formats = formats
            .iter()
            .filter(|f| f.include_when_renaming && ids.contains(&f.id))
            .map(|f| f.name.clone())
            .collect::<Vec<_>>();
        let naming_file = NamingFile {
            quality_title: Some(c.quality.quality.name.to_owned()),
            revision_version: c.quality.revision.version as i32,
            revision_real: c.quality.revision.real as i32,
            release_group: c.release_group.clone(),
            scene_name: Some(c.scene_name.clone()).filter(|s| !s.is_empty()),
            relative_path: None,
            path: Some(c.path.to_string_lossy().into_owned()),
            edition: None,
            media: c
                .probe
                .as_ref()
                .and_then(|p| crate::media::naming_media(&p.raw, Some(&c.scene_name))),
            custom_formats,
            release_hash: c.release_hash.clone(),
        };
        let episodes: Vec<NamingEpisode> = c.episodes.iter().map(naming_episode).collect();
        let name = build_episode_file_name(
            &naming_series(series),
            &episodes,
            &naming_file,
            extension.len(),
            &self.config.naming,
        )?;
        let first = &c.episodes[0];
        let mut relative = String::new();
        if series.season_folder {
            let title = series
                .season(first.season_number)
                .and_then(|s| s.title.clone());
            relative = build_season_folder(
                &naming_series(series),
                first.season_number,
                title.as_deref(),
                &self.config.naming,
            );
            relative.push('/');
        }
        relative.push_str(&name);
        relative.push_str(&extension);
        let destination = Path::new(&series.path).join(&relative);
        if destination == c.path {
            return Err("File not moved, source and destination are the same".into());
        }
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
        }

        // Old files for these episodes go away only once the new one is in place.
        let files = self
            .db
            .episode_files()
            .for_series(series.id)
            .await
            .map_err(|e| e.to_string())?;
        let mut old: Vec<EpisodeFileRecord> = Vec::new();
        for id in c.episodes.iter().filter_map(|e| e.episode_file_id) {
            if let Some(f) = files.iter().find(|f| f.id == id)
                && !old.iter().any(|o| o.id == id)
            {
                old.push(f.clone());
            }
        }
        let mut displaced: Vec<(PathBuf, PathBuf)> = Vec::new();
        for o in &old {
            let p = Path::new(&series.path).join(&o.relative_path);
            if p == destination {
                let backup = destination
                    .with_extension(format!("{}.omp-old", extension.trim_start_matches('.')));
                tokio::fs::rename(&p, &backup)
                    .await
                    .map_err(|e| e.to_string())?;
                displaced.push((p, backup));
            }
        }
        if destination.exists() {
            return Err(format!(
                "Destination already exists: {}",
                destination.display()
            ));
        }
        let copy_only = match self.config.mode {
            ImportMode::Auto => !req.can_move_files,
            ImportMode::Move => false,
            ImportMode::Copy => true,
        };
        if let Err(e) = transfer(
            &c.path,
            &destination,
            copy_only,
            self.config.copy_using_hardlinks,
        )
        .await
        {
            for (p, backup) in &displaced {
                let _ = tokio::fs::rename(backup, p).await;
            }
            return Err(format!("Failed to transfer file: {e}"));
        }
        for o in &old {
            let p = Path::new(&series.path).join(&o.relative_path);
            let actual = displaced
                .iter()
                .find(|(orig, _)| *orig == p)
                .map_or(p.clone(), |(_, b)| b.clone());
            let name = Path::new(&o.relative_path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if let Err(e) = crate::recycle::dispose(self.config, &actual, &name).await {
                tracing::warn!(file = %actual.display(), "could not remove the replaced file: {e}");
            }
            self.db
                .episode_files()
                .delete(o.id)
                .await
                .map_err(|e| e.to_string())?;
        }

        let flags = self.grab_flags(req).await.unwrap_or_default();
        let record = EpisodeFileRecord {
            id: 0,
            series_id: series.id,
            season_number: first.season_number,
            relative_path: relative,
            size: c.size,
            date_added: req.now,
            scene_name: Some(c.scene_name.clone()).filter(|s| !s.is_empty()),
            release_group: c.release_group.clone(),
            quality: c.quality,
            languages: c.languages.clone(),
            media_info: c.probe.as_ref().map(|p| p.raw.clone()),
            original_file_path: c
                .path
                .strip_prefix(req.output_path.parent().unwrap_or(req.output_path))
                .ok()
                .map(|p| p.to_string_lossy().into_owned()),
            indexer_flags: flags,
        };
        let file_id = self
            .db
            .episode_files()
            .insert(&record)
            .await
            .map_err(|e| e.to_string())?;
        let ids: Vec<i64> = c.episodes.iter().map(|e| e.id).collect();
        self.db
            .episodes()
            .set_file(&ids, Some(file_id))
            .await
            .map_err(|e| e.to_string())?;
        for e in &c.episodes {
            self.db
                .episode_history()
                .insert(&EpisodeHistoryRecord {
                    id: 0,
                    series_id: series.id,
                    episode_id: e.id,
                    event: HistoryEvent::DownloadFolderImported,
                    date: req.now,
                    source_title: c
                        .path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    quality: c.quality,
                    languages: c.languages.clone(),
                    download_id: req.download_id.map(str::to_owned),
                    data: [
                        (
                            "imported_path".to_owned(),
                            destination.to_string_lossy().into_owned(),
                        ),
                        (
                            "dropped_path".to_owned(),
                            c.path.to_string_lossy().into_owned(),
                        ),
                    ]
                    .into(),
                })
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(destination)
    }

    async fn grab_flags(
        &self,
        req: &TvImportRequest<'_>,
    ) -> Option<omp_core::release::IndexerFlags> {
        let history = self
            .db
            .episode_history()
            .by_download_id(req.download_id?)
            .await
            .ok()?;
        let grab = history.iter().find(|h| h.event == HistoryEvent::Grabbed)?;
        Some(omp_core::release::IndexerFlags(
            grab.data.get("indexer_flags")?.parse().ok()?,
        ))
    }
}

/// Used by the manual import screens to see how a folder would match.
pub async fn match_files_to_episodes(
    db: &Db,
    series_id: i64,
    names: &[String],
) -> Result<Vec<Vec<EpisodeRecord>>, ServiceError> {
    let all = db.episodes().for_series(series_id).await?;
    Ok(names
        .iter()
        .map(|n| {
            parse_title(n)
                .filter(|p| !p.full_season)
                .and_then(|p| p.season_number().map(|s| pick_episodes(&p, &all, s)))
                .unwrap_or_default()
        })
        .collect())
}
