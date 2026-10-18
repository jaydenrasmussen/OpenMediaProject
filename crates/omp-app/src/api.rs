//! The HTTP API, under `/api/v1`.

use std::{convert::Infallible, path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use omp_core::{history::HistoryEvent, movie::MovieStatus};
use omp_db::{
    DbError,
    repo::{HistoryRecord, MovieFileRecord, MovieRecord, NewMovie, Provider, ProviderKind},
};
use omp_naming::{NamingMovie, build_movie_folder};
use omp_quality::profile::QualityProfile;
use omp_services::{SearchMode, load_import_config};
use serde::Deserialize;
use serde_json::{Value, json};
use tower::{ServiceExt, service_fn};
use tower_http::services::{ServeDir, ServeFile};

use crate::{app::App, tasks};

type Shared = State<Arc<App>>;
pub(crate) type ApiResult<T = Json<Value>> = Result<T, ApiError>;

#[derive(Debug)]
pub struct ApiError(pub(crate) StatusCode, pub(crate) String);

impl ApiError {
    pub(crate) fn internal(m: impl Into<String>) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, m.into())
    }
    pub(crate) fn bad_request(m: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, m.into())
    }
    fn not_found(m: impl Into<String>) -> Self {
        Self(StatusCode::NOT_FOUND, m.into())
    }
    fn unavailable(m: impl Into<String>) -> Self {
        Self(StatusCode::SERVICE_UNAVAILABLE, m.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<DbError> for ApiError {
    fn from(e: DbError) -> Self {
        match e {
            DbError::NotFound => Self(StatusCode::NOT_FOUND, "not found".into()),
            DbError::Conflict(m) => Self(StatusCode::CONFLICT, m),
            DbError::Constraint(m) => Self(StatusCode::CONFLICT, m),
            other => {
                tracing::error!("database error: {other}");
                Self(StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
            }
        }
    }
}

impl From<omp_services::ServiceError> for ApiError {
    fn from(e: omp_services::ServiceError) -> Self {
        tracing::error!("service error: {e}");
        Self(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

// ---- JSON shapes -------------------------------------------------------------------------

fn status_name(s: MovieStatus) -> &'static str {
    match s {
        MovieStatus::Deleted => "deleted",
        MovieStatus::Tba => "tba",
        MovieStatus::Announced => "announced",
        MovieStatus::InCinemas => "inCinemas",
        MovieStatus::Released => "released",
    }
}

fn parse_status(s: &str) -> Option<MovieStatus> {
    match s {
        "tba" => Some(MovieStatus::Tba),
        "announced" => Some(MovieStatus::Announced),
        "inCinemas" => Some(MovieStatus::InCinemas),
        "released" => Some(MovieStatus::Released),
        _ => None,
    }
}

fn file_json(f: &MovieFileRecord) -> Value {
    json!({
        "id": f.id,
        "relative_path": f.relative_path,
        "size": f.size,
        "date_added": f.date_added,
        "quality": f.quality.quality.name,
        "release_group": f.release_group,
        "scene_name": f.scene_name,
        "edition": f.edition,
    })
}

fn movie_json(m: &MovieRecord, file: Option<&MovieFileRecord>) -> Value {
    json!({
        "id": m.movie.id,
        "tmdb_id": m.movie.tmdb_id,
        "imdb_id": m.metadata.imdb_id,
        "title": m.movie.title,
        "original_title": m.metadata.original_title,
        "year": m.movie.year,
        "overview": m.metadata.overview,
        "runtime": m.metadata.runtime,
        "status": status_name(m.metadata.status),
        "genres": m.metadata.genres,
        "certification": m.metadata.certification,
        "collection": m.metadata.collection_title,
        "images": m.metadata.images,
        "monitored": m.movie.monitored,
        "minimum_availability": status_name(m.movie.minimum_availability),
        "quality_profile_id": m.movie.quality_profile_id,
        "path": m.movie.path,
        "root_folder_path": m.root_folder_path,
        "tags": m.movie.tags,
        "added": m.added,
        "last_search_time": m.last_search_time,
        "has_file": file.is_some(),
        "file": file.map(file_json),
    })
}

fn event_name(e: HistoryEvent) -> &'static str {
    match e {
        HistoryEvent::Unknown => "unknown",
        HistoryEvent::Grabbed => "grabbed",
        HistoryEvent::DownloadFolderImported => "downloadFolderImported",
        HistoryEvent::DownloadFailed => "downloadFailed",
        HistoryEvent::MovieFileDeleted => "movieFileDeleted",
        HistoryEvent::MovieFolderImported => "movieFolderImported",
        HistoryEvent::MovieFileRenamed => "movieFileRenamed",
        HistoryEvent::DownloadIgnored => "downloadIgnored",
    }
}

fn history_json(h: &HistoryRecord) -> Value {
    json!({
        "id": h.id,
        "movie_id": h.movie_id,
        "event": event_name(h.event),
        "date": h.date,
        "source_title": h.source_title,
        "quality": h.quality.quality.name,
        "download_id": h.download_id,
        "data": h.data,
    })
}

fn provider_json(p: &Provider) -> Value {
    json!({
        "id": p.id,
        "name": p.name,
        "implementation": p.implementation,
        "plugin_id": p.plugin_id,
        "settings": p.settings,
        "tags": p.tags,
        "enabled": p.enabled,
        "enable_rss": p.enable_rss,
        "enable_automatic_search": p.enable_automatic_search,
        "enable_interactive_search": p.enable_interactive_search,
        "priority": p.priority,
    })
}

// ---- handlers ----------------------------------------------------------------------------

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

async fn health_checks(State(app): Shared) -> ApiResult {
    let issues = crate::health::check(&app).await?;
    Ok(Json(
        serde_json::to_value(issues).map_err(|e| ApiError::internal(e.to_string()))?,
    ))
}

async fn list_backups(State(app): Shared) -> ApiResult {
    let backups = crate::backups::list(&app)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(
        serde_json::to_value(backups).map_err(|e| ApiError::internal(e.to_string()))?,
    ))
}

async fn create_backup(State(app): Shared) -> ApiResult<(StatusCode, Json<Value>)> {
    let b = crate::backups::create(&app)
        .await
        .map_err(ApiError::internal)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(b).map_err(|e| ApiError::internal(e.to_string()))?),
    ))
}

async fn download_backup(
    State(app): Shared,
    Path(name): Path<String>,
) -> Result<Response, ApiError> {
    let path = crate::backups::path_of(&app, &name)
        .ok_or_else(|| ApiError::not_found("no such backup"))?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| ApiError::not_found("no such backup"))?;
    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/vnd.sqlite3".to_owned(),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

async fn delete_backup(State(app): Shared, Path(name): Path<String>) -> ApiResult<StatusCode> {
    let path = crate::backups::path_of(&app, &name)
        .ok_or_else(|| ApiError::not_found("no such backup"))?;
    tokio::fs::remove_file(path)
        .await
        .map_err(|_| ApiError::not_found("no such backup"))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn system_status(State(app): Shared) -> Json<Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "started": app.started,
        "tmdb_configured": app.metadata.is_some(),
        "data_dir": app.config.data_dir,
    }))
}

async fn list_movies(State(app): Shared) -> ApiResult {
    let movies = app.db.movies().list().await?;
    let mut out = Vec::with_capacity(movies.len());
    for m in &movies {
        let file = app.db.movie_files().for_movie(m.movie.id).await?;
        out.push(movie_json(m, file.as_ref()));
    }
    Ok(Json(Value::Array(out)))
}

async fn get_movie(State(app): Shared, Path(id): Path<i64>) -> ApiResult {
    let m = app.db.movies().get(id).await?;
    let file = app.db.movie_files().for_movie(id).await?;
    Ok(Json(movie_json(&m, file.as_ref())))
}

#[derive(Deserialize)]
struct AddMovie {
    tmdb_id: i64,
    quality_profile_id: i32,
    root_folder_path: String,
    #[serde(default = "yes")]
    monitored: bool,
    minimum_availability: Option<String>,
    #[serde(default)]
    tags: Vec<i32>,
    /// Search for the movie right away.
    #[serde(default)]
    search_now: bool,
}

fn yes() -> bool {
    true
}

async fn add_movie(
    State(app): Shared,
    Json(req): Json<AddMovie>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let Some(provider) = app.metadata.clone() else {
        return Err(ApiError::unavailable(
            "no metadata provider is configured (set TMDB_READ_TOKEN)",
        ));
    };
    if !app
        .db
        .root_folders()
        .list()
        .await?
        .iter()
        .any(|r| r.path == req.root_folder_path)
    {
        return Err(ApiError::bad_request(
            "root_folder_path is not a configured root folder",
        ));
    }
    let minimum = match req.minimum_availability.as_deref() {
        None => MovieStatus::Announced,
        Some(s) => parse_status(s)
            .ok_or_else(|| ApiError::bad_request(format!("unknown minimum_availability '{s}'")))?,
    };
    app.db
        .quality_profiles()
        .get(req.quality_profile_id)
        .await
        .map_err(|_| ApiError::bad_request("unknown quality_profile_id"))?;

    if app
        .db
        .movies()
        .find_by_tmdb_id(req.tmdb_id)
        .await?
        .is_some()
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "this movie is already in the library".into(),
        ));
    }

    let meta = provider.movie(req.tmdb_id).await.map_err(|e| match e {
        omp_metadata::MetadataError::NotFound => ApiError::not_found("TMDB has no such movie"),
        other => ApiError(StatusCode::BAD_GATEWAY, other.to_string()),
    })?;

    let naming = load_import_config(&app.db).await?.naming;
    let folder = build_movie_folder(
        &NamingMovie {
            title: meta.title.clone(),
            original_title: Some(meta.original_title.clone()),
            year: meta.year,
            imdb_id: meta.imdb_id.clone(),
            tmdb_id: meta.tmdb_id,
            certification: meta.certification.clone(),
            collection_title: meta.collection_title.clone(),
            translations: Vec::new(),
        },
        &naming,
    );
    let path = PathBuf::from(&req.root_folder_path).join(folder);

    let metadata_id = app.db.movies().upsert_metadata(&meta).await?;
    let id = app
        .db
        .movies()
        .add(&NewMovie {
            metadata_id,
            monitored: req.monitored,
            minimum_availability: minimum,
            quality_profile_id: req.quality_profile_id,
            path: path.to_string_lossy().into_owned(),
            root_folder_path: Some(req.root_folder_path),
            tags: req.tags,
        })
        .await?;
    let record = app.db.movies().get(id).await?;
    if req.search_now && req.monitored {
        let app = app.clone();
        tokio::spawn(async move {
            if let Err(e) = tasks::search_movie_once(&app, id, SearchMode::Automatic, true).await {
                tracing::warn!("search for new movie failed: {e}");
            }
        });
    }
    Ok((StatusCode::CREATED, Json(movie_json(&record, None))))
}

#[derive(Deserialize)]
struct UpdateMovie {
    monitored: Option<bool>,
    minimum_availability: Option<String>,
    quality_profile_id: Option<i32>,
    path: Option<String>,
    tags: Option<Vec<i32>>,
}

async fn update_movie(
    State(app): Shared,
    Path(id): Path<i64>,
    Json(req): Json<UpdateMovie>,
) -> ApiResult {
    let current = app.db.movies().get(id).await?;
    let minimum = match req.minimum_availability.as_deref() {
        None => current.movie.minimum_availability,
        Some(s) => parse_status(s)
            .ok_or_else(|| ApiError::bad_request(format!("unknown minimum_availability '{s}'")))?,
    };
    if let Some(p) = req.quality_profile_id {
        app.db
            .quality_profiles()
            .get(p)
            .await
            .map_err(|_| ApiError::bad_request("unknown quality_profile_id"))?;
    }
    app.db
        .movies()
        .update(
            id,
            &omp_db::repo::MovieUpdate {
                monitored: req.monitored.unwrap_or(current.movie.monitored),
                minimum_availability: minimum,
                quality_profile_id: req
                    .quality_profile_id
                    .unwrap_or(current.movie.quality_profile_id),
                path: req.path.unwrap_or(current.movie.path.clone()),
                root_folder_path: current.root_folder_path.clone(),
                tags: req.tags.unwrap_or(current.movie.tags.clone()),
            },
        )
        .await?;
    get_movie(State(app), Path(id)).await
}

async fn delete_movie(State(app): Shared, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    app.db.movies().delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn queue(State(app): Shared) -> Json<Value> {
    let items: Vec<Value> = app
        .monitor
        .snapshot()
        .into_iter()
        .map(|t| {
            json!({
                "download_id": t.item.download_id,
                "title": t.item.title,
                "movie_id": t.movie_id,
                "state": format!("{:?}", t.state),
                "status": format!("{:?}", t.item.status),
                "client": t.client_name,
                "protocol": format!("{:?}", t.protocol),
                "indexer": t.indexer,
                "size": t.item.total_size,
                "size_left": t.item.remaining_size,
                "added": t.added,
                "warnings": t.warnings,
                "message": t.item.message,
            })
        })
        .collect();
    Json(Value::Array(items))
}

#[derive(Deserialize)]
struct Paging {
    #[serde(default = "default_page_size")]
    limit: i64,
    #[serde(default)]
    offset: i64,
    /// Only this movie's events.
    movie_id: Option<i64>,
}

fn default_page_size() -> i64 {
    50
}

async fn history(State(app): Shared, Query(p): Query<Paging>) -> ApiResult {
    let rows = match p.movie_id {
        Some(id) => app.db.history().for_movie(id).await?,
        None => {
            app.db
                .history()
                .recent(p.limit.clamp(1, 500), p.offset.max(0))
                .await?
        }
    };
    Ok(Json(Value::Array(rows.iter().map(history_json).collect())))
}

#[derive(Deserialize)]
struct Command {
    name: String,
    movie_id: Option<i64>,
    /// Run to completion before answering (for scripts and tests).
    #[serde(default)]
    wait: bool,
}

async fn command(
    State(app): Shared,
    Json(cmd): Json<Command>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let job = {
        let app = app.clone();
        let name = cmd.name.clone();
        let movie_id = cmd.movie_id;
        async move {
            match name.as_str() {
                "RssSync" => tasks::rss_sync_once(&app)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                "ProcessDownloads" => tasks::process_downloads_once(&app)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                "MoviesSearch" => match movie_id {
                    Some(id) => tasks::search_movie_once(&app, id, SearchMode::Automatic, true)
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                    None => Err("MoviesSearch needs a movie_id".into()),
                },
                other => Err(format!("unknown command '{other}'")),
            }
        }
    };
    if !matches!(
        cmd.name.as_str(),
        "RssSync" | "ProcessDownloads" | "MoviesSearch"
    ) {
        return Err(ApiError::bad_request(format!(
            "unknown command '{}'",
            cmd.name
        )));
    }
    if cmd.name == "MoviesSearch" && cmd.movie_id.is_none() {
        return Err(ApiError::bad_request("MoviesSearch needs a movie_id"));
    }
    if cmd.wait {
        return match job.await {
            Ok(()) => Ok((
                StatusCode::OK,
                Json(json!({ "name": cmd.name, "status": "completed" })),
            )),
            Err(e) => Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, e)),
        };
    }
    let name = cmd.name.clone();
    tokio::spawn(async move {
        if let Err(e) = job.await {
            tracing::warn!(command = %name, "command failed: {e}");
        }
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "name": cmd.name, "status": "queued" })),
    ))
}

// ---- lookup and interactive search -------------------------------------------------------

#[derive(Deserialize)]
struct Lookup {
    term: String,
}

fn lookup_json(m: &omp_core::movie::MovieMetadata, in_library: Option<i64>) -> Value {
    json!({
        "tmdb_id": m.tmdb_id,
        "imdb_id": m.imdb_id,
        "title": m.title,
        "year": m.year,
        "overview": m.overview,
        "runtime": m.runtime,
        "images": m.images,
        "in_library": in_library,
    })
}

/// Search TMDB. `tmdb:603` and `imdb:tt0133093` look one movie up directly;
/// anything else is a title search, with an optional trailing year.
async fn movie_lookup(State(app): Shared, Query(q): Query<Lookup>) -> ApiResult {
    let Some(provider) = app.metadata.clone() else {
        return Err(ApiError::unavailable(
            "no metadata provider is configured (set TMDB_READ_TOKEN)",
        ));
    };
    let term = q.term.trim();
    if term.is_empty() {
        return Err(ApiError::bad_request("term is required"));
    }
    let found: Vec<omp_core::movie::MovieMetadata> = if let Some(id) = term.strip_prefix("tmdb:") {
        let id: i64 = id
            .trim()
            .parse()
            .map_err(|_| ApiError::bad_request("tmdb: needs a number"))?;
        vec![provider.movie(id).await.map_err(metadata_error)?]
    } else if let Some(id) = term.strip_prefix("imdb:") {
        vec![
            provider
                .movie_by_imdb_id(id.trim())
                .await
                .map_err(metadata_error)?,
        ]
    } else {
        let (title, year) = split_year(term);
        provider.search(title, year).await.map_err(metadata_error)?
    };
    let mut out = Vec::new();
    for m in &found {
        let existing = app
            .db
            .movies()
            .find_by_tmdb_id(m.tmdb_id)
            .await?
            .map(|r| r.movie.id);
        out.push(lookup_json(m, existing));
    }
    Ok(Json(Value::Array(out)))
}

fn metadata_error(e: omp_metadata::MetadataError) -> ApiError {
    match e {
        omp_metadata::MetadataError::NotFound => ApiError::not_found("no such movie"),
        other => ApiError(StatusCode::BAD_GATEWAY, other.to_string()),
    }
}

/// "The Matrix 1999" or "The Matrix (1999)" -> ("The Matrix", Some(1999)).
fn split_year(term: &str) -> (&str, Option<i32>) {
    let trimmed = term.trim_end_matches(')').trim_end();
    if let Some((head, tail)) = trimmed.rsplit_once(|c: char| c.is_whitespace() || c == '(')
        && tail.len() == 4
        && let Ok(year) = tail.parse::<i32>()
        && (1870..=2200).contains(&year)
    {
        let head = head.trim_end_matches('(').trim();
        if !head.is_empty() {
            return (head, Some(year));
        }
    }
    (term, None)
}

#[derive(Deserialize)]
struct ReleaseSearch {
    movie_id: i64,
}

/// Searches every interactive-search indexer for a movie and returns what was
/// found with the decision engine's verdict on each. Nothing is grabbed.
async fn search_releases(State(app): Shared, Query(q): Query<ReleaseSearch>) -> ApiResult {
    app.db.movies().get(q.movie_id).await?;
    let result = tasks::search_movie_once(&app, q.movie_id, SearchMode::Interactive, true).await?;
    let failures: Vec<Value> = result
        .failures
        .iter()
        .map(|f| json!({ "indexer": f.indexer, "error": f.error.to_string() }))
        .collect();

    // Best approved releases first, in the engine's order; the rest by size.
    let mut rank: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (n, i) in result
        .evaluation
        .approved_best_first()
        .into_iter()
        .flatten()
        .enumerate()
    {
        rank.insert(i, n + 1);
    }
    let (search_id, evaluation) = app.search_cache.lock().unwrap().insert(result.evaluation);
    let now = chrono::Utc::now();
    let mut items: Vec<(usize, Value)> = evaluation
        .items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let r = &it.release;
            (
                i,
                json!({
                    "id": format!("{search_id}-{i}"),
                    "guid": r.guid,
                    "title": r.title,
                    "indexer": r.indexer,
                    "protocol": format!("{:?}", r.protocol).to_lowercase(),
                    "size": r.size,
                    "age_hours": r.age_hours(now),
                    "seeders": r.torrent.as_ref().and_then(|t| t.seeders),
                    "peers": r.torrent.as_ref().and_then(|t| t.peers),
                    "quality": it.parsed.as_ref().map(|p| p.quality.quality.name),
                    "release_group": it.parsed.as_ref().and_then(|p| p.release_group.clone()),
                    "custom_format_score": it.custom_format_score,
                    "matched_movie_id": it.movie.as_ref().map(|m| m.movie.id),
                    "approved": it.decision.approved(),
                    "temporarily_rejected": it.decision.temporarily_rejected(),
                    "rejections": it.decision.rejections.iter().map(|x| x.message.clone()).collect::<Vec<_>>(),
                    "rank": rank.get(&i),
                }),
            )
        })
        .collect();
    items.sort_by_key(|(i, v)| {
        (
            rank.get(i).copied().unwrap_or(usize::MAX),
            std::cmp::Reverse(v["size"].as_u64().unwrap_or(0)),
        )
    });
    Ok(Json(json!({
        "releases": items.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        "indexer_failures": failures,
    })))
}

#[derive(Deserialize)]
struct GrabRelease {
    /// An `id` from a release search.
    id: String,
}

async fn grab_release(State(app): Shared, Json(req): Json<GrabRelease>) -> ApiResult {
    let (search, index) = req
        .id
        .split_once('-')
        .and_then(|(s, i)| Some((s.parse::<u64>().ok()?, i.parse::<usize>().ok()?)))
        .ok_or_else(|| ApiError::bad_request("malformed release id"))?;
    let evaluation = app
        .search_cache
        .lock()
        .unwrap()
        .get(search)
        .ok_or_else(|| ApiError::not_found("those search results have expired; search again"))?;
    if index >= evaluation.items.len() {
        return Err(ApiError::bad_request("no such release in those results"));
    }
    let ctx = omp_services::GrabContext {
        now: chrono::Utc::now(),
        process_start: app.started,
        category: Some(app.config.download_category.clone()),
    };
    let grabbed = omp_services::grab_selected(&app.db, &evaluation, &app.registry(), &ctx, index)
        .await
        .map_err(|m| ApiError(StatusCode::BAD_GATEWAY, m))?;
    Ok(Json(json!({
        "download_id": grabbed.download_id,
        "client": grabbed.client_name,
        "title": evaluation.items[index].release.title,
    })))
}

// ---- settings ----------------------------------------------------------------------------

async fn list_root_folders(State(app): Shared) -> ApiResult {
    let rows = app.db.root_folders().list().await?;
    Ok(Json(Value::Array(
        rows.iter()
            .map(|r| json!({ "id": r.id, "path": r.path }))
            .collect(),
    )))
}

#[derive(Deserialize)]
struct NewRoot {
    path: String,
}

async fn add_root_folder(
    State(app): Shared,
    Json(req): Json<NewRoot>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let path = std::path::Path::new(&req.path);
    if !path.is_absolute() {
        return Err(ApiError::bad_request("the path must be absolute"));
    }
    if !path.is_dir() {
        return Err(ApiError::bad_request("that folder does not exist"));
    }
    let r = app
        .db
        .root_folders()
        .add(req.path.trim_end_matches('/'))
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": r.id, "path": r.path })),
    ))
}

async fn delete_root_folder(State(app): Shared, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    app.db.root_folders().delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_profiles(State(app): Shared) -> ApiResult {
    let profiles = app.db.quality_profiles().list().await?;
    Ok(Json(
        serde_json::to_value(profiles).map_err(|e| ApiError::bad_request(e.to_string()))?,
    ))
}

async fn get_profile(State(app): Shared, Path(id): Path<i32>) -> ApiResult {
    let p = app.db.quality_profiles().get(id).await?;
    Ok(Json(
        serde_json::to_value(p).map_err(|e| ApiError::bad_request(e.to_string()))?,
    ))
}

async fn add_profile(
    State(app): Shared,
    Json(mut p): Json<QualityProfile>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    p.id = 0;
    p.validate().map_err(ApiError::bad_request)?;
    let id = app.db.quality_profiles().insert(&p).await?;
    let saved = app.db.quality_profiles().get(id).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(saved).map_err(|e| ApiError::bad_request(e.to_string()))?),
    ))
}

async fn update_profile(
    State(app): Shared,
    Path(id): Path<i32>,
    Json(mut p): Json<QualityProfile>,
) -> ApiResult {
    p.id = id;
    p.validate().map_err(ApiError::bad_request)?;
    app.db.quality_profiles().update(&p).await?;
    get_profile(State(app), Path(id)).await
}

async fn delete_profile(State(app): Shared, Path(id): Path<i32>) -> ApiResult<StatusCode> {
    app.db.quality_profiles().delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ProviderBody {
    name: String,
    implementation: String,
    plugin_id: Option<String>,
    #[serde(default)]
    settings: Value,
    #[serde(default)]
    tags: Vec<i32>,
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(default = "yes")]
    enable_rss: bool,
    #[serde(default = "yes")]
    enable_automatic_search: bool,
    #[serde(default = "yes")]
    enable_interactive_search: bool,
    #[serde(default = "default_priority")]
    priority: i32,
}

fn default_priority() -> i32 {
    25
}

impl ProviderBody {
    fn into_provider(self, id: i64, kind: ProviderKind) -> Provider {
        Provider {
            id,
            kind,
            name: self.name,
            implementation: self.implementation,
            plugin_id: self.plugin_id,
            settings: if self.settings.is_null() {
                json!({})
            } else {
                self.settings
            },
            tags: self.tags,
            enabled: self.enabled,
            enable_rss: self.enable_rss,
            enable_automatic_search: self.enable_automatic_search,
            enable_interactive_search: self.enable_interactive_search,
            priority: self.priority,
        }
    }
}

fn validate_provider(kind: ProviderKind, p: &Provider) -> Result<(), ApiError> {
    if p.name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    let known: &[&str] = match kind {
        ProviderKind::Indexer => &["newznab", "torznab"],
        ProviderKind::DownloadClient => &["torrent", "usenet"],
        _ => return Ok(()),
    };
    // Plugin-provided implementations are validated by the plugin host.
    if p.plugin_id.is_none() && !known.contains(&p.implementation.as_str()) {
        return Err(ApiError::bad_request(format!(
            "unknown implementation '{}'",
            p.implementation
        )));
    }
    Ok(())
}

async fn list_providers(app: Arc<App>, kind: ProviderKind) -> ApiResult {
    let rows = app.db.providers().list(kind).await?;
    Ok(Json(Value::Array(rows.iter().map(provider_json).collect())))
}

async fn save_provider(
    app: Arc<App>,
    kind: ProviderKind,
    id: Option<i64>,
    body: ProviderBody,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let mut p = body.into_provider(id.unwrap_or(0), kind);
    validate_provider(kind, &p)?;
    let status = match id {
        Some(id) => {
            let existing = app.db.providers().get(id).await?;
            if existing.kind != kind {
                return Err(ApiError::not_found("not found"));
            }
            app.db.providers().update(&p).await?;
            StatusCode::OK
        }
        None => {
            p.id = app.db.providers().insert(&p).await?;
            StatusCode::CREATED
        }
    };
    if kind == ProviderKind::DownloadClient {
        app.reload_clients()
            .await
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    Ok((status, Json(provider_json(&p))))
}

async fn delete_provider(app: Arc<App>, kind: ProviderKind, id: i64) -> ApiResult<StatusCode> {
    if app.db.providers().get(id).await?.kind != kind {
        return Err(ApiError::not_found("not found"));
    }
    app.db.providers().delete(id).await?;
    if kind == ProviderKind::DownloadClient {
        app.reload_clients()
            .await
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Tries a provider's settings without saving them.
async fn test_provider(app: Arc<App>, kind: ProviderKind, body: ProviderBody) -> ApiResult {
    let p = body.into_provider(0, kind);
    validate_provider(kind, &p)?;
    let outcome = match kind {
        ProviderKind::Indexer => omp_services::test_indexer(&p).await,
        ProviderKind::DownloadClient => crate::clients::test_client(&p, &app.config).await,
        _ => Ok(()),
    };
    Ok(Json(match outcome {
        Ok(()) => json!({ "ok": true }),
        Err(error) => json!({ "ok": false, "error": error }),
    }))
}

macro_rules! provider_routes {
    ($router:expr, $path:literal, $kind:expr) => {{
        let item = concat!($path, "/{id}");
        let test = concat!($path, "/test");
        $router
            .route(
                test,
                post(|State(app): Shared, Json(b): Json<ProviderBody>| {
                    test_provider(app, $kind, b)
                }),
            )
            .route(
                $path,
                get(|State(app): Shared| list_providers(app, $kind)).post(
                    |State(app): Shared, Json(b): Json<ProviderBody>| {
                        save_provider(app, $kind, None, b)
                    },
                ),
            )
            .route(
                item,
                axum::routing::put(
                    |State(app): Shared, Path(id): Path<i64>, Json(b): Json<ProviderBody>| {
                        save_provider(app, $kind, Some(id), b)
                    },
                )
                .delete(|State(app): Shared, Path(id): Path<i64>| delete_provider(app, $kind, id)),
            )
    }};
}

async fn get_import_config(State(app): Shared) -> ApiResult {
    let c = load_import_config(&app.db).await?;
    Ok(Json(
        serde_json::to_value(c).map_err(|e| ApiError::bad_request(e.to_string()))?,
    ))
}

async fn put_import_config(
    State(app): Shared,
    Json(c): Json<omp_services::ImportConfig>,
) -> ApiResult {
    omp_services::save_import_config(&app.db, &c).await?;
    get_import_config(State(app)).await
}

/// The router: the API plus the static UI as the fallback.
pub fn router(app: Arc<App>) -> Router {
    let ui_dir = app.config.ui_dir.clone();
    // Static export: unknown *page* paths fall back to the SPA shell. Paths that
    // look like files (have an extension) must 404, or a missing JS/CSS asset
    // would be served as HTML with a 200.
    let shell = ServeFile::new(ui_dir.join("index.html"));
    let ui = ServeDir::new(&ui_dir).fallback(service_fn(move |req: Request<Body>| {
        let shell = shell.clone();
        async move {
            let last = req.uri().path().rsplit('/').next().unwrap_or_default();
            if last.contains('.') {
                Ok(StatusCode::NOT_FOUND.into_response())
            } else {
                Ok::<_, Infallible>(shell.oneshot(req).await.into_response())
            }
        }
    }));

    let api = Router::new()
        .route("/system/status", get(system_status))
        .route("/system/health", get(health_checks))
        .route("/system/backup", get(list_backups).post(create_backup))
        .route(
            "/system/backup/{name}",
            get(download_backup).delete(delete_backup),
        )
        .route("/auth/logout", post(crate::auth::logout))
        .route("/auth/password", post(crate::auth::change_password))
        .route("/auth/apikey", get(crate::auth::get_api_key))
        .route(
            "/auth/apikey/regenerate",
            post(crate::auth::regenerate_api_key),
        )
        .route("/movie", get(list_movies).post(add_movie))
        .route("/movie/lookup", get(movie_lookup))
        .route("/release", get(search_releases).post(grab_release))
        .route(
            "/movie/{id}",
            get(get_movie).put(update_movie).delete(delete_movie),
        )
        .route("/queue", get(queue))
        .route("/history", get(history))
        .route("/command", post(command))
        .route("/rootfolder", get(list_root_folders).post(add_root_folder))
        .route(
            "/rootfolder/{id}",
            axum::routing::delete(delete_root_folder),
        )
        .route("/qualityprofile", get(list_profiles).post(add_profile))
        .route(
            "/qualityprofile/{id}",
            get(get_profile).put(update_profile).delete(delete_profile),
        )
        .route(
            "/config/import",
            get(get_import_config).put(put_import_config),
        );
    let api = provider_routes!(api, "/indexer", ProviderKind::Indexer);
    let api = provider_routes!(api, "/downloadclient", ProviderKind::DownloadClient);

    // Everything above needs a login session or the API key.
    let api = api.route_layer(axum::middleware::from_fn_with_state(
        app.clone(),
        crate::auth::require_auth,
    ));
    let public = Router::new()
        .route("/health", get(health))
        .route("/auth/status", get(crate::auth::status))
        .route("/auth/setup", post(crate::auth::setup))
        .route("/auth/login", post(crate::auth::login));
    // Unknown API paths are errors, not the UI shell.
    let api = api
        .merge(public)
        .fallback(|| async { ApiError::not_found("no such API endpoint") });
    Router::new()
        .nest("/api/v1", api)
        .fallback_service(ui)
        .with_state(app)
}

#[cfg(test)]
mod tests {
    use super::split_year;

    #[test]
    fn years_are_split_off_search_terms() {
        assert_eq!(split_year("The Matrix 1999"), ("The Matrix", Some(1999)));
        assert_eq!(split_year("The Matrix (1999)"), ("The Matrix", Some(1999)));
        assert_eq!(
            split_year("2001"),
            ("2001", None),
            "a title that is only a number"
        );
        assert_eq!(
            split_year("Blade Runner 2049"),
            ("Blade Runner", Some(2049))
        );
        assert_eq!(split_year("Alien"), ("Alien", None));
        assert_eq!(split_year("Apollo 13"), ("Apollo 13", None));
    }
}
