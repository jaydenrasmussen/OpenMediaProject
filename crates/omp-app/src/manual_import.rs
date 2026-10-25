//! Manual import: a person points at files in a finished download (usually
//! one that stopped at "import blocked") and says what they are.

use std::{
    path::{Path as FsPath, PathBuf},
    sync::Arc,
};

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::get,
};
use chrono::Utc;
use omp_services::{
    FileOutcome, ImportRequest, Importer, ManualImport, SeriesIndex, TrackedState, TvImportRequest,
    TvImporter, list_media_files, load_import_config, match_release,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{ApiError, ApiResult, Shared},
    app::App,
};

pub(crate) fn routes() -> Router<Arc<App>> {
    Router::new().route("/manualimport", get(list).post(run))
}

/// The folder a request is about: a tracked download's output, or a path
/// inside the downloads folder.
fn resolve_folder(
    app: &App,
    download_id: Option<&str>,
    path: Option<&str>,
) -> Result<PathBuf, ApiError> {
    if let Some(id) = download_id {
        let tracked = app
            .monitor
            .get(id)
            .ok_or_else(|| ApiError::not_found("that download isn't tracked"))?;
        let out = tracked
            .item
            .output_path
            .ok_or_else(|| ApiError::bad_request("the download client gave no output path"))?;
        return Ok(PathBuf::from(out));
    }
    let path = path.ok_or_else(|| ApiError::bad_request("give a download_id or a path"))?;
    Ok(PathBuf::from(path))
}

/// Files may only come from the downloads folder or a tracked download's
/// output, never from elsewhere on the disk.
fn inside_allowed(app: &App, path: &FsPath) -> bool {
    let Ok(real) = path.canonicalize() else {
        return false;
    };
    let mut roots = vec![app.config.downloads_dir.clone()];
    roots.extend(
        app.monitor
            .snapshot()
            .into_iter()
            .filter_map(|t| t.item.output_path.map(PathBuf::from)),
    );
    roots
        .iter()
        .filter_map(|r| r.canonicalize().ok())
        .any(|r| real.starts_with(r))
}

#[derive(Deserialize)]
struct ListQuery {
    download_id: Option<String>,
    path: Option<String>,
}

/// The media files in a download with what the library thinks they are.
async fn list(State(app): Shared, Query(q): Query<ListQuery>) -> ApiResult {
    let folder = resolve_folder(&app, q.download_id.as_deref(), q.path.as_deref())?;
    if !inside_allowed(&app, &folder) {
        return Err(ApiError::bad_request(
            "that folder is not inside the downloads folder",
        ));
    }
    let files = list_media_files(&folder)
        .await
        .map_err(|e| ApiError::bad_request(format!("cannot read the folder: {e}")))?;
    let library = omp_library::Library::new(&app.db);
    let index = SeriesIndex::load(&app.db).await?;
    let mut out = Vec::new();
    for file in files {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let size = tokio::fs::metadata(&file)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        let relative = file
            .strip_prefix(&folder)
            .unwrap_or(&file)
            .to_string_lossy()
            .into_owned();
        let mut movie = Value::Null;
        let mut series = Value::Null;
        let mut episodes: Vec<Value> = Vec::new();
        let mut quality = None;

        // A name that reads as an episode is an episode; otherwise try movies.
        let tv = omp_parser::tv::parse_path(&relative).filter(|p| {
            !p.series_title.trim().is_empty()
                && (!p.episode_numbers.is_empty() || p.is_daily() || p.is_absolute_numbering())
        });
        if let Some(parsed) = &tv {
            quality = Some(parsed.quality.quality.name);
            if let Some(m) =
                match_release(&app.db, &index, parsed, &Default::default(), None).await?
            {
                series = json!({ "id": m.series.id, "title": m.series.info.title });
                episodes = m
                    .episodes
                    .iter()
                    .map(|e| json!({ "id": e.id, "season_number": e.season_number, "episode_number": e.episode_number, "title": e.title }))
                    .collect();
            }
        }
        if series.is_null()
            && let Some(parsed) = omp_parser::title::parse_movie_title(&name, false)
        {
            quality = quality.or(Some(parsed.quality.quality.name));
            if let Ok(Some(m)) = library.match_release(&parsed, None, 0, None).await {
                movie = json!({ "id": m.movie.movie.id, "title": m.movie.movie.title, "year": m.movie.movie.year });
            }
        }
        out.push(json!({
            "path": file.to_string_lossy(),
            "name": name,
            "relative_path": relative,
            "size": size,
            "quality": quality,
            "movie": movie,
            "series": series,
            "episodes": episodes,
        }));
    }
    Ok(Json(Value::Array(out)))
}

#[derive(Deserialize)]
struct Item {
    path: String,
    movie_id: Option<i64>,
    series_id: Option<i64>,
    #[serde(default)]
    episode_ids: Vec<i64>,
}

#[derive(Deserialize)]
struct Run {
    items: Vec<Item>,
    /// The tracked download these files came from; it is marked imported
    /// when every file went in.
    download_id: Option<String>,
}

async fn run(State(app): Shared, Json(req): Json<Run>) -> ApiResult<(StatusCode, Json<Value>)> {
    if req.items.is_empty() || req.items.len() > 500 {
        return Err(ApiError::bad_request("choose between 1 and 500 files"));
    }
    for item in &req.items {
        match (item.movie_id, item.series_id) {
            (Some(_), None) => {}
            (None, Some(_)) if !item.episode_ids.is_empty() => {}
            (None, Some(_)) => {
                return Err(ApiError::bad_request(
                    "choose the episodes for each series file",
                ));
            }
            _ => {
                return Err(ApiError::bad_request(
                    "each file needs a movie, or a series with its episodes",
                ));
            }
        }
        if !inside_allowed(&app, FsPath::new(&item.path)) {
            return Err(ApiError::bad_request(format!(
                "{} is not inside the downloads folder",
                item.path
            )));
        }
    }

    let config = load_import_config(&app.db).await?;
    let tracked = req
        .download_id
        .as_deref()
        .and_then(|id| app.monitor.get(id));
    let can_move = tracked.as_ref().is_some_and(|t| t.item.can_move_files);
    let title = tracked
        .as_ref()
        .map(|t| t.item.title.clone())
        .unwrap_or_default();
    let now = Utc::now();
    let movies = Importer {
        db: &app.db,
        config: &config,
        probe: app.probe.as_ref(),
        disk: app.disk.as_ref(),
    };
    let shows = TvImporter {
        db: &app.db,
        config: &config,
        probe: app.probe.as_ref(),
        disk: app.disk.as_ref(),
    };

    let mut results = Vec::new();
    let mut all_ok = true;
    for item in &req.items {
        let path = FsPath::new(&item.path);
        let result = if let Some(movie_id) = item.movie_id {
            movies
                .import(&ImportRequest {
                    movie_id,
                    output_path: path,
                    download_id: req.download_id.as_deref(),
                    download_title: &title,
                    can_move_files: can_move,
                    now,
                    manual: Some(ManualImport::default()),
                })
                .await
        } else {
            shows
                .import(&TvImportRequest {
                    series_id: item.series_id.unwrap_or_default(),
                    output_path: path,
                    download_id: req.download_id.as_deref(),
                    download_title: &title,
                    episode_ids: &item.episode_ids,
                    can_move_files: can_move,
                    now,
                    manual: Some(ManualImport::default()),
                })
                .await
        };
        let entry = match result {
            Ok(r) => match r.files.into_iter().next().map(|f| f.outcome) {
                Some(FileOutcome::Imported { to }) => {
                    json!({ "path": item.path, "ok": true, "imported_to": to.to_string_lossy() })
                }
                Some(FileOutcome::Failed(m)) => {
                    json!({ "path": item.path, "ok": false, "error": m })
                }
                Some(FileOutcome::Rejected(rs)) => {
                    json!({ "path": item.path, "ok": false, "error": rs.iter().map(|r| r.to_string()).collect::<Vec<_>>().join("; ") })
                }
                None => json!({ "path": item.path, "ok": false, "error": "not a media file" }),
            },
            Err(e) => json!({ "path": item.path, "ok": false, "error": e.to_string() }),
        };
        all_ok &= entry["ok"] == true;
        results.push(entry);
    }
    if all_ok
        && let Some(id) = req.download_id.as_deref()
        && tracked.is_some()
    {
        app.monitor.set_state(id, TrackedState::Imported);
    }
    Ok((StatusCode::OK, Json(json!({ "results": results }))))
}
