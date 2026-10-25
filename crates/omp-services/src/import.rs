//! Importing a finished download into the library. Ported from Radarr
//! `DownloadedMovieImportService`, `ImportDecisionMaker` (and its
//! specifications), `ImportApprovedMovie` and `MovieFileMovingService`
//! (GPL-3.0).

use std::{
    cmp::Ordering,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use omp_core::{
    history::HistoryEvent,
    quality::{Quality, QualityModel},
};
use omp_db::{
    Db,
    repo::{HistoryRecord, MovieFileRecord, MovieRecord},
};
use omp_naming::{NamingConfig, NamingFile, NamingMovie, build_file_name};
use omp_parser::{files::is_media_extension, title::parse_movie_title};
use serde::{Deserialize, Serialize};

use crate::{
    DiskSpace,
    evaluate::ServiceError,
    formats::{input_for_file, matching_ids},
};

/// What the media probe (ffprobe) reports for a file.
#[derive(Debug, Clone, Default)]
pub struct ProbeInfo {
    pub runtime_seconds: f64,
    pub audio_streams: u32,
    /// The full probe output, stored with the file.
    pub raw: serde_json::Value,
}

#[async_trait]
pub trait MediaProbe: Send + Sync {
    /// `None` when the file cannot be read as media.
    async fn probe(&self, path: &Path) -> Option<ProbeInfo>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportMode {
    /// Move unless the download client says the files are still needed.
    Auto,
    Move,
    Copy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ImportConfig {
    pub mode: ImportMode,
    /// When copying, hardlink if the file system allows it.
    pub copy_using_hardlinks: bool,
    pub minimum_free_space_mb: u64,
    pub skip_free_space_check: bool,
    /// Prefer propers and repacks over the same quality.
    pub download_propers: bool,
    /// Folder name prefixes where download clients unpack, separated by `|`.
    pub working_folders: String,
    /// Where replaced and deleted files go instead of being erased. Empty
    /// means they are deleted.
    pub recycle_bin: String,
    /// Days files stay in the bin. 0 keeps them until someone empties it.
    pub recycle_bin_cleanup_days: u32,
    /// Bring subtitles and other files named after a video along with it.
    pub import_extra_files: bool,
    /// Write Kodi `.nfo` files and poster/fanart pictures beside the media.
    pub write_metadata: bool,
    /// Which ones, comma separated (`srt,ass`).
    pub extra_file_extensions: String,
    /// Stop monitoring a movie once its file is deleted by hand.
    pub unmonitor_deleted: bool,
    /// Look for another release when a download fails.
    pub redownload_failed: bool,
    pub naming: NamingConfig,
}

impl Default for ImportConfig {
    fn default() -> Self {
        Self {
            mode: ImportMode::Auto,
            copy_using_hardlinks: true,
            minimum_free_space_mb: 100,
            skip_free_space_check: false,
            download_propers: true,
            working_folders: "_UNPACK_|_FAILED_".into(),
            recycle_bin: String::new(),
            recycle_bin_cleanup_days: 7,
            import_extra_files: false,
            write_metadata: false,
            extra_file_extensions: "srt,ass,ssa,sub,idx".into(),
            unmonitor_deleted: false,
            redownload_failed: true,
            naming: NamingConfig::default(),
        }
    }
}

/// Why a file was not imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportRejection {
    Sample,
    SampleIndeterminate,
    NoAudio,
    MultiPart,
    Unpacking,
    AlreadyImported(DateTime<Utc>),
    NotQualityUpgrade(String),
    NotRevisionUpgrade,
    NotCustomFormatUpgrade {
        new: i32,
        current: i32,
    },
    NotEnoughFreeSpace,
    /// A better file for the same movie was chosen.
    AlreadyImportedThisRun,
    /// Any other reason, already worded.
    Other(String),
}

impl std::fmt::Display for ImportRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sample => write!(f, "Sample"),
            Self::SampleIndeterminate => write!(f, "Unable to determine if file is a sample"),
            Self::NoAudio => write!(f, "No audio tracks detected"),
            Self::MultiPart => {
                write!(
                    f,
                    "File is suspected multi-part file, multi-part movies are not supported"
                )
            }
            Self::Unpacking => write!(f, "File is still being unpacked"),
            Self::AlreadyImported(at) => write!(f, "Movie file already imported at {at}"),
            Self::NotQualityUpgrade(msg) => write!(f, "{msg}"),
            Self::NotRevisionUpgrade => {
                write!(
                    f,
                    "Not a quality revision upgrade for existing movie file(s)"
                )
            }
            Self::NotCustomFormatUpgrade { new, current } => write!(
                f,
                "Not a Custom Format upgrade for existing movie file(s). New score {new} does not improve on existing {current}"
            ),
            Self::NotEnoughFreeSpace => write!(f, "Not enough free space"),
            Self::AlreadyImportedThisRun => write!(f, "Already imported in this run"),
            Self::Other(message) => write!(f, "{message}"),
        }
    }
}

#[derive(Debug)]
pub enum FileOutcome {
    Imported { to: PathBuf },
    Rejected(Vec<ImportRejection>),
    Failed(String),
}

#[derive(Debug)]
pub struct FileResult {
    pub path: PathBuf,
    pub outcome: FileOutcome,
}

#[derive(Debug, Default)]
pub struct ImportResult {
    pub files: Vec<FileResult>,
}

impl ImportResult {
    pub fn imported(&self) -> bool {
        self.files
            .iter()
            .any(|f| matches!(f.outcome, FileOutcome::Imported { .. }))
    }
}

/// A person chose these files and what they are, so the usual checks (sample,
/// upgrade, already imported...) are skipped. Only a quality can be forced.
#[derive(Debug, Clone, Default)]
pub struct ManualImport {
    pub quality: Option<QualityModel>,
}

pub struct ImportRequest<'a> {
    pub movie_id: i64,
    /// A file, or the folder the download client put its output in.
    pub output_path: &'a Path,
    pub download_id: Option<&'a str>,
    /// The title the download had (used when a file name says nothing).
    pub download_title: &'a str,
    /// The client still needs the files (seeding), so copy instead of move.
    pub can_move_files: bool,
    pub now: DateTime<Utc>,
    pub manual: Option<ManualImport>,
}

pub struct Importer<'a> {
    pub db: &'a Db,
    pub config: &'a ImportConfig,
    pub probe: &'a dyn MediaProbe,
    pub disk: &'a dyn DiskSpace,
}

struct Candidate {
    path: PathBuf,
    size: u64,
    quality: QualityModel,
    release_group: Option<String>,
    edition: String,
    scene_name: String,
    languages: Vec<omp_core::language::Language>,
    probe: Option<ProbeInfo>,
}

pub(crate) fn extension_of(path: &Path) -> String {
    path.extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default()
}

/// Every media file at or under `root`, for screens that list a download.
pub async fn list_media_files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    find_media_files(root).await
}

pub(crate) async fn find_media_files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let meta = tokio::fs::metadata(root).await?;
    if meta.is_file() {
        return Ok(if is_media_extension(&extension_of(root)) {
            vec![root.to_path_buf()]
        } else {
            vec![]
        });
    }
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() && is_media_extension(&extension_of(&path)) {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Smallest runtime that counts as a real movie rather than a sample.
pub(crate) fn minimum_runtime_seconds(runtime_minutes: u32) -> f64 {
    match runtime_minutes {
        0..=3 => 15.0,
        4..=10 => 90.0,
        11..=30 => 300.0,
        _ => 600.0,
    }
}

fn is_multi_part(path: &Path, siblings: &[PathBuf]) -> bool {
    use std::sync::LazyLock;

    use omp_parser::re::Re;
    static PATTERNS: LazyLock<[Re; 2]> = LazyLock::new(|| {
        [
            Re::ci(r"(?<!^)(?<identifier>[ _.-]*(?:cd|dvd|p(?:(?:ar)?t)?|dis[ck])[ _.-]*[0-9]+)"),
            Re::ci(r"(?<!^)(?<identifier>[ _.-]*(?:cd|dvd|p(?:(?:ar)?t)?|dis[ck])[ _.-]*[a-d]+)"),
        ]
    });
    let this = path.to_string_lossy();
    if !PATTERNS.iter().any(|re| re.is_match(&this)) {
        return false;
    }
    let same_dir: Vec<String> = siblings
        .iter()
        .filter(|s| s.parent() == path.parent())
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    PATTERNS.iter().any(|re| {
        let stripped = re.replace_all(&this, "");
        same_dir
            .iter()
            .filter(|f| re.replace_all(f, "") == stripped)
            .count()
            > 1
    })
}

pub(crate) fn in_working_folder(path: &Path, working_folders: &str) -> bool {
    let folders: Vec<&str> = working_folders
        .split('|')
        .filter(|f| !f.is_empty())
        .collect();
    path.parent().is_some_and(|parent| {
        parent.ancestors().any(|a| {
            a.file_name()
                .map(|n| n.to_string_lossy())
                .is_some_and(|n| folders.iter().any(|f| n.starts_with(f)))
        })
    })
}

impl Importer<'_> {
    pub async fn import(&self, req: &ImportRequest<'_>) -> Result<ImportResult, ServiceError> {
        let movie = self.db.movies().get(req.movie_id).await?;
        let profile = self
            .db
            .quality_profiles()
            .get(movie.movie.quality_profile_id)
            .await?;
        let formats = self.db.custom_formats().load_compiled().await?;
        let existing = self.db.movie_files().for_movie(movie.movie.id).await?;
        let history = match req.download_id {
            Some(id) => self.db.history().by_download_id(id).await?,
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

        let mut result = ImportResult::default();
        let mut approved: Vec<Candidate> = Vec::new();

        for path in &paths {
            let mut rejections = Vec::new();
            let mut candidate = self.describe(path, req).await;
            if let Some(q) = req.manual.as_ref().and_then(|m| m.quality) {
                candidate.quality = q;
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
                        && p.runtime_seconds < minimum_runtime_seconds(movie.metadata.runtime)
                    {
                        rejections.push(ImportRejection::Sample);
                    }
                    if p.audio_streams == 0 {
                        rejections.push(ImportRejection::NoAudio);
                    }
                }
            }
            if is_multi_part(path, &paths) {
                rejections.push(ImportRejection::MultiPart);
            }
            if existing.is_some()
                && let Some(r) = self.already_imported(&history, req)
            {
                rejections.push(r);
            }
            if !self.config.skip_free_space_check {
                let dir = Path::new(&movie.movie.path)
                    .parent()
                    .unwrap_or(Path::new("/"));
                if let Some(free) = self.disk.free_bytes(&dir.to_string_lossy())
                    && free < candidate.size + self.config.minimum_free_space_mb * 1024 * 1024
                {
                    rejections.push(ImportRejection::NotEnoughFreeSpace);
                }
            }
            if let Some(existing) = &existing
                && let Some(r) =
                    self.upgrade_rejection(&candidate, existing, &movie, &profile, &formats)
            {
                rejections.push(r);
            }

            if req.manual.is_some() {
                rejections.clear();
            }
            if rejections.is_empty() {
                approved.push(candidate);
            } else {
                result.files.push(FileResult {
                    path: path.clone(),
                    outcome: FileOutcome::Rejected(rejections),
                });
            }
        }

        // Best first: quality, then size. Only one file per movie is kept.
        approved.sort_by(|a, b| {
            profile
                .comparer()
                .compare_model(&b.quality, &a.quality)
                .then(b.size.cmp(&a.size))
        });
        let mut done = false;
        for candidate in approved {
            if done {
                result.files.push(FileResult {
                    path: candidate.path,
                    outcome: FileOutcome::Rejected(vec![ImportRejection::AlreadyImportedThisRun]),
                });
                continue;
            }
            let outcome = match self.import_file(&movie, &candidate, req, &formats).await {
                Ok(to) => {
                    done = true;
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

    async fn describe(&self, path: &Path, req: &ImportRequest<'_>) -> Candidate {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let from_file = parse_movie_title(&file_name, false);
        let from_download = parse_movie_title(req.download_title, false);

        // The file name wins; the download's own name fills what it lacks.
        let mut quality = from_file.as_ref().map(|p| p.quality).unwrap_or_default();
        if quality.quality == Quality::UNKNOWN
            && let Some(d) = &from_download
        {
            quality = d.quality;
        }
        let release_group = from_file
            .as_ref()
            .and_then(|p| p.release_group.clone())
            .or_else(|| from_download.as_ref().and_then(|p| p.release_group.clone()));
        let edition = from_file
            .as_ref()
            .map(|p| p.edition.clone())
            .filter(|e| !e.is_empty())
            .or_else(|| from_download.as_ref().map(|p| p.edition.clone()))
            .unwrap_or_default();
        let languages = from_file
            .as_ref()
            .or(from_download.as_ref())
            .map(|p| p.languages.clone())
            .unwrap_or_default();
        let scene_name = if from_download.is_some() {
            req.download_title.to_owned()
        } else {
            String::new()
        };
        let size = tokio::fs::metadata(path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        Candidate {
            path: path.to_path_buf(),
            size,
            quality,
            release_group,
            edition,
            scene_name,
            languages,
            probe: self.probe.probe(path).await,
        }
    }

    fn already_imported(
        &self,
        history: &[HistoryRecord],
        _req: &ImportRequest<'_>,
    ) -> Option<ImportRejection> {
        let last_imported = history
            .iter()
            .filter(|h| h.event == HistoryEvent::DownloadFolderImported)
            .map(|h| h.date)
            .max()?;
        match history
            .iter()
            .filter(|h| h.event == HistoryEvent::Grabbed)
            .map(|h| h.date)
            .max()
        {
            // Grabbed again after the import: a new download.
            Some(grabbed) if grabbed > last_imported => None,
            _ => Some(ImportRejection::AlreadyImported(last_imported)),
        }
    }

    fn upgrade_rejection(
        &self,
        new: &Candidate,
        existing: &MovieFileRecord,
        movie: &MovieRecord,
        profile: &omp_quality::profile::QualityProfile,
        formats: &[omp_quality::custom_format::CustomFormat],
    ) -> Option<ImportRejection> {
        let quality_compare = profile
            .comparer()
            .compare_quality(new.quality.quality, existing.quality.quality);
        if quality_compare == Ordering::Less {
            return Some(ImportRejection::NotQualityUpgrade(format!(
                "Not an upgrade for existing movie file. Existing quality: {}. New quality: {}",
                existing.quality.quality.name, new.quality.quality.name
            )));
        }
        if quality_compare == Ordering::Equal
            && self.config.download_propers
            && new.quality.revision.partial_cmp(&existing.quality.revision) == Some(Ordering::Less)
        {
            return Some(ImportRejection::NotRevisionUpgrade);
        }
        let current_ids = matching_ids(formats, &input_for_file(existing, movie));
        let current = profile.calculate_custom_format_score(&current_ids);
        let new_input = crate::formats::input_for_candidate(
            new.quality.quality,
            &new.edition,
            new.release_group.as_deref(),
            &new.scene_name,
            &new.path,
            new.size,
            &new.languages,
            movie,
        );
        let new_score = profile.calculate_custom_format_score(&matching_ids(formats, &new_input));
        (quality_compare == Ordering::Equal && new_score < current).then_some(
            ImportRejection::NotCustomFormatUpgrade {
                new: new_score,
                current,
            },
        )
    }

    async fn import_file(
        &self,
        movie: &MovieRecord,
        c: &Candidate,
        req: &ImportRequest<'_>,
        formats: &[omp_quality::custom_format::CustomFormat],
    ) -> Result<PathBuf, String> {
        let extension = extension_of(&c.path);
        let definitions_title = c.quality.quality.name.to_owned();
        let custom_formats = {
            let input = crate::formats::input_for_candidate(
                c.quality.quality,
                &c.edition,
                c.release_group.as_deref(),
                &c.scene_name,
                &c.path,
                c.size,
                &c.languages,
                movie,
            );
            let ids = matching_ids(formats, &input);
            formats
                .iter()
                .filter(|f| f.include_when_renaming && ids.contains(&f.id))
                .map(|f| f.name.clone())
                .collect::<Vec<_>>()
        };

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
            quality_title: Some(definitions_title),
            revision_version: c.quality.revision.version as i32,
            revision_real: c.quality.revision.real as i32,
            release_group: c.release_group.clone(),
            scene_name: Some(c.scene_name.clone()).filter(|s| !s.is_empty()),
            relative_path: c.path.file_name().map(|n| n.to_string_lossy().into_owned()),
            path: Some(c.path.to_string_lossy().into_owned()),
            edition: Some(c.edition.clone()).filter(|e| !e.is_empty()),
            media: c
                .probe
                .as_ref()
                .and_then(|p| crate::media::naming_media(&p.raw, Some(&c.scene_name))),
            custom_formats,
            release_hash: None,
        };
        let name = build_file_name(&naming_movie, &naming_file, &self.config.naming)?;
        let relative = format!("{name}{extension}");
        let destination = Path::new(&movie.movie.path).join(&relative);

        if destination == c.path {
            return Err("File not moved, source and destination are the same".into());
        }
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
        }

        // The old file goes away only once the new one is in place.
        let old = self
            .db
            .movie_files()
            .for_movie(movie.movie.id)
            .await
            .map_err(|e| e.to_string())?;
        let old_path = old
            .as_ref()
            .map(|o| Path::new(&movie.movie.path).join(&o.relative_path));
        let displaced = match &old_path {
            Some(p) if *p == destination => {
                let backup = destination
                    .with_extension(format!("{}.omp-old", extension.trim_start_matches('.')));
                tokio::fs::rename(p, &backup)
                    .await
                    .map_err(|e| e.to_string())?;
                Some(backup)
            }
            _ => None,
        };
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
            if let (Some(backup), Some(p)) = (&displaced, &old_path) {
                let _ = tokio::fs::rename(backup, p).await;
            }
            return Err(format!("Failed to transfer file: {e}"));
        }
        crate::extras::import(self.config, &c.path, &destination, copy_only).await;

        // The replaced file goes to the recycle bin, or is erased.
        let replaced = displaced.or(old_path.clone());
        if let Some(p) = replaced {
            let name = old
                .as_ref()
                .map(|o| o.relative_path.clone())
                .unwrap_or_else(|| {
                    p.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                });
            let name = Path::new(&name)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if let Err(e) = crate::recycle::dispose(self.config, &p, &name).await {
                tracing::warn!(file = %p.display(), "could not remove the replaced file: {e}");
            }
        }

        let flags = req
            .download_id
            .and(self.grab_flags(req).await)
            .unwrap_or_default();
        let record = MovieFileRecord {
            id: 0,
            movie_id: movie.movie.id,
            relative_path: relative,
            size: c.size,
            date_added: req.now,
            scene_name: Some(c.scene_name.clone()).filter(|s| !s.is_empty()),
            release_group: c.release_group.clone(),
            edition: Some(c.edition.clone()).filter(|e| !e.is_empty()),
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
        self.db
            .movie_files()
            .set(&record)
            .await
            .map_err(|e| e.to_string())?;
        self.db
            .history()
            .insert(&HistoryRecord {
                id: 0,
                movie_id: movie.movie.id,
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
        Ok(destination)
    }

    async fn grab_flags(&self, req: &ImportRequest<'_>) -> Option<omp_core::release::IndexerFlags> {
        let history = self
            .db
            .history()
            .by_download_id(req.download_id?)
            .await
            .ok()?;
        let grab = history.iter().rfind(|h| h.event == HistoryEvent::Grabbed)?;
        Some(omp_core::release::IndexerFlags(
            grab.data.get("indexer_flags")?.parse().ok()?,
        ))
    }
}

/// Moves, copies or hardlinks `from` to `to`.
pub(crate) async fn transfer(
    from: &Path,
    to: &Path,
    copy_only: bool,
    hardlink: bool,
) -> std::io::Result<()> {
    if copy_only {
        if hardlink && tokio::fs::hard_link(from, to).await.is_ok() {
            return Ok(());
        }
        copy_atomically(from, to).await
    } else {
        match tokio::fs::rename(from, to).await {
            Ok(()) => Ok(()),
            // Different file systems: copy, then remove the original.
            Err(_) => {
                copy_atomically(from, to).await?;
                tokio::fs::remove_file(from).await
            }
        }
    }
}

/// Copies to a temporary name beside `to` and renames it into place, so
/// `to` is never seen half written and an interrupted copy leaves nothing
/// under the real name.
async fn copy_atomically(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut partial = to.as_os_str().to_owned();
    partial.push(".partial");
    let partial = std::path::PathBuf::from(partial);
    if let Err(e) = tokio::fs::copy(from, &partial).await {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(e);
    }
    tokio::fs::rename(&partial, to).await
}

#[cfg(test)]
mod transfer_tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    async fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.mkv");
        tokio::fs::write(&from, b"movie").await.unwrap();
        let to = dir.path().join("b.mkv");
        (dir, from, to)
    }

    #[tokio::test]
    async fn hardlinking_shares_the_file_and_keeps_the_original() {
        let (_d, from, to) = fixture().await;
        transfer(&from, &to, true, true).await.unwrap();
        assert_eq!(
            std::fs::metadata(&from).unwrap().ino(),
            std::fs::metadata(&to).unwrap().ino()
        );
    }

    #[tokio::test]
    async fn copying_without_hardlinks_makes_a_separate_file() {
        let (_d, from, to) = fixture().await;
        transfer(&from, &to, true, false).await.unwrap();
        assert_ne!(
            std::fs::metadata(&from).unwrap().ino(),
            std::fs::metadata(&to).unwrap().ino()
        );
        assert_eq!(std::fs::read(&to).unwrap(), b"movie");
        assert!(from.exists());
    }

    #[tokio::test]
    async fn moving_removes_the_original() {
        let (_d, from, to) = fixture().await;
        transfer(&from, &to, false, false).await.unwrap();
        assert!(!from.exists());
        assert_eq!(std::fs::read(&to).unwrap(), b"movie");
    }

    #[tokio::test]
    async fn an_atomic_copy_leaves_no_partial_file_and_fails_cleanly() {
        let (d, from, to) = fixture().await;
        copy_atomically(&from, &to).await.unwrap();
        assert!(!d.path().join("b.mkv.partial").exists());
        let missing = d.path().join("nope.mkv");
        assert!(
            copy_atomically(&missing, &d.path().join("c.mkv"))
                .await
                .is_err()
        );
        assert!(!d.path().join("c.mkv").exists() && !d.path().join("c.mkv.partial").exists());
    }
}
