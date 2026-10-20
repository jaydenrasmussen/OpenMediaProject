//! The TV half of the API: series, seasons and episodes.

use std::{path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post, put},
};
use chrono::Utc;
use omp_core::tv::{MonitorMode, NewItemMonitor, SeriesInfo, SeriesType};
use omp_db::repo::{EpisodeFileRecord, EpisodeRecord, SeriesRecord, SeriesUpdate};
use omp_naming::{NamingSeries, build_series_folder};
use omp_services::{AddSeries, add_series, apply_monitor_mode, load_import_config, refresh_series};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{ApiError, ApiResult, Shared, is_inside_a_root, metadata_error, split_year},
    app::App,
};

pub(crate) fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/series", get(list_series).post(add_series_handler))
        .route("/series/lookup", get(series_lookup))
        .route(
            "/series/{id}",
            get(get_series).put(update_series).delete(delete_series),
        )
        .route("/series/{id}/season/{season}", put(set_season_monitored))
        .route("/episode", get(list_episodes))
        .route("/episode/monitor", put(monitor_episodes))
        .route("/episode/{id}", get(get_episode))
        .route(
            "/episodefile/{id}",
            axum::routing::delete(delete_episode_file),
        )
        .route("/series/{id}/monitor", post(apply_monitor))
}

// ---- JSON shapes -----------------------------------------------------------

fn file_json(f: &EpisodeFileRecord) -> Value {
    json!({
        "id": f.id,
        "relative_path": f.relative_path,
        "size": f.size,
        "date_added": f.date_added,
        "quality": f.quality.quality.name,
        "release_group": f.release_group,
        "scene_name": f.scene_name,
    })
}

fn episode_json(e: &EpisodeRecord, file: Option<&EpisodeFileRecord>) -> Value {
    json!({
        "id": e.id,
        "series_id": e.series_id,
        "season_number": e.season_number,
        "episode_number": e.episode_number,
        "absolute_episode_number": e.absolute_episode_number,
        "title": e.title,
        "overview": e.overview,
        "air_date": e.air_date,
        "air_date_utc": e.air_date_utc,
        "runtime": e.runtime,
        "monitored": e.monitored,
        "has_file": e.has_file(),
        "file": file.map(file_json),
        "last_search_time": e.last_search_time,
        "finale_type": e.finale_type,
    })
}

fn type_name(t: SeriesType) -> &'static str {
    match t {
        SeriesType::Standard => "standard",
        SeriesType::Daily => "daily",
        SeriesType::Anime => "anime",
    }
}

fn status_name(s: omp_core::tv::SeriesStatus) -> &'static str {
    match s {
        omp_core::tv::SeriesStatus::Continuing => "continuing",
        omp_core::tv::SeriesStatus::Ended => "ended",
        omp_core::tv::SeriesStatus::Upcoming => "upcoming",
        omp_core::tv::SeriesStatus::Deleted => "deleted",
    }
}

/// Episode and file counts for a series or one of its seasons.
#[derive(Default, Clone, Copy)]
struct Counts {
    /// Every episode the provider lists.
    total: usize,
    /// Episodes that are monitored or already have a file: what "complete" is measured against.
    episodes: usize,
    monitored: usize,
    with_files: usize,
    aired: usize,
    aired_missing: usize,
    size: u64,
}

fn count(episodes: &[&EpisodeRecord], files: &[EpisodeFileRecord]) -> Counts {
    let now = Utc::now();
    let mut c = Counts::default();
    let mut seen_files = std::collections::HashSet::new();
    for e in episodes {
        c.total += 1;
        c.episodes += usize::from(e.monitored || e.has_file());
        c.monitored += usize::from(e.monitored);
        c.with_files += usize::from(e.has_file());
        let aired = e.has_aired(now);
        c.aired += usize::from(aired);
        c.aired_missing += usize::from(aired && e.monitored && !e.has_file());
        if let Some(id) = e.episode_file_id
            && seen_files.insert(id)
            && let Some(f) = files.iter().find(|f| f.id == id)
        {
            c.size += f.size;
        }
    }
    c
}

fn counts_json(c: Counts) -> Value {
    json!({
        "episode_count": c.episodes,
        "total_episode_count": c.total,
        "monitored_episode_count": c.monitored,
        "episode_file_count": c.with_files,
        "aired_episode_count": c.aired,
        "missing_episode_count": c.aired_missing,
        "size_on_disk": c.size,
    })
}

fn series_json(s: &SeriesRecord, episodes: &[EpisodeRecord], files: &[EpisodeFileRecord]) -> Value {
    let all: Vec<&EpisodeRecord> = episodes.iter().collect();
    let seasons: Vec<Value> = s
        .seasons
        .iter()
        .map(|season| {
            let mine: Vec<&EpisodeRecord> = episodes
                .iter()
                .filter(|e| e.season_number == season.season_number)
                .collect();
            let mut v = counts_json(count(&mine, files));
            v["season_number"] = json!(season.season_number);
            v["monitored"] = json!(season.monitored);
            v["title"] = json!(season.title);
            v["images"] = json!(season.images);
            v
        })
        .collect();
    let now = Utc::now();
    let next_airing = episodes
        .iter()
        .filter(|e| e.monitored && e.air_date_utc.is_some_and(|d| d > now))
        .min_by_key(|e| e.air_date_utc)
        .map(|e| e.air_date_utc);
    let i = &s.info;
    let mut v = counts_json(count(&all, files));
    let obj = v.as_object_mut().expect("an object");
    for (k, val) in [
        ("id", json!(s.id)),
        ("tmdb_id", json!(i.tmdb_id)),
        ("tvdb_id", json!(i.tvdb_id)),
        ("imdb_id", json!(i.imdb_id)),
        ("title", json!(i.title)),
        ("original_title", json!(i.original_title)),
        ("year", json!(i.year)),
        ("overview", json!(i.overview)),
        ("status", json!(status_name(i.status))),
        ("network", json!(i.network)),
        ("runtime", json!(i.runtime)),
        ("genres", json!(i.genres)),
        ("certification", json!(i.certification)),
        ("first_aired", json!(i.first_aired)),
        ("last_aired", json!(i.last_aired)),
        ("images", json!(i.images)),
        ("series_type", json!(type_name(s.series_type))),
        ("monitored", json!(s.monitored)),
        (
            "monitor_new_items",
            json!(match s.monitor_new_items {
                NewItemMonitor::All => "all",
                NewItemMonitor::None => "none",
            }),
        ),
        ("quality_profile_id", json!(s.quality_profile_id)),
        ("season_folder", json!(s.season_folder)),
        ("use_scene_numbering", json!(s.use_scene_numbering)),
        ("path", json!(s.path)),
        ("root_folder_path", json!(s.root_folder_path)),
        ("tags", json!(s.tags)),
        ("added", json!(s.added)),
        ("last_info_sync", json!(s.last_info_sync)),
        ("seasons", json!(seasons)),
        ("next_airing", json!(next_airing)),
    ] {
        obj.insert(k.to_owned(), val);
    }
    v
}

// ---- reading ----------------------------------------------------------------

async fn list_series(State(app): Shared) -> ApiResult {
    let series = app.db.series().list().await?;
    let mut out = Vec::with_capacity(series.len());
    for s in &series {
        let episodes = app.db.episodes().for_series(s.id).await?;
        let files = app.db.episode_files().for_series(s.id).await?;
        out.push(series_json(s, &episodes, &files));
    }
    Ok(Json(Value::Array(out)))
}

async fn get_series(State(app): Shared, Path(id): Path<i64>) -> ApiResult {
    let s = app.db.series().get(id).await?;
    let episodes = app.db.episodes().for_series(id).await?;
    let files = app.db.episode_files().for_series(id).await?;
    Ok(Json(series_json(&s, &episodes, &files)))
}

#[derive(Deserialize)]
struct EpisodeQuery {
    series_id: i64,
    season: Option<i32>,
}

async fn list_episodes(State(app): Shared, Query(q): Query<EpisodeQuery>) -> ApiResult {
    app.db.series().get(q.series_id).await?;
    let episodes = app.db.episodes().for_series(q.series_id).await?;
    let files = app.db.episode_files().for_series(q.series_id).await?;
    Ok(Json(Value::Array(
        episodes
            .iter()
            .filter(|e| q.season.is_none_or(|s| e.season_number == s))
            .map(|e| {
                let file = e
                    .episode_file_id
                    .and_then(|id| files.iter().find(|f| f.id == id));
                episode_json(e, file)
            })
            .collect(),
    )))
}

async fn get_episode(State(app): Shared, Path(id): Path<i64>) -> ApiResult {
    let e = app.db.episodes().get(id).await?;
    let file = match e.episode_file_id {
        Some(f) => Some(app.db.episode_files().get(f).await?),
        None => None,
    };
    Ok(Json(episode_json(&e, file.as_ref())))
}

// ---- lookup -----------------------------------------------------------------

#[derive(Deserialize)]
struct Lookup {
    term: String,
}

fn lookup_json(s: &SeriesInfo, in_library: Option<i64>) -> Value {
    json!({
        "tmdb_id": s.tmdb_id,
        "tvdb_id": s.tvdb_id,
        "imdb_id": s.imdb_id,
        "title": s.title,
        "year": s.year,
        "overview": s.overview,
        "status": status_name(s.status),
        "network": s.network,
        "images": s.images,
        "suggested_type": type_name(s.suggested_type),
        "in_library": in_library,
    })
}

/// Search the metadata provider. `tmdb:1396`, `tvdb:81189` and `imdb:tt0903747`
/// look one series up directly; anything else is a title search.
async fn series_lookup(State(app): Shared, Query(q): Query<Lookup>) -> ApiResult {
    let Some(provider) = app.series_metadata.clone() else {
        return Err(ApiError::unavailable(
            "no metadata provider is configured (set TMDB_READ_TOKEN)",
        ));
    };
    let term = q.term.trim();
    if term.is_empty() {
        return Err(ApiError::bad_request("term is required"));
    }
    let number = |text: &str, what: &str| -> Result<i64, ApiError> {
        text.trim()
            .parse()
            .map_err(|_| ApiError::bad_request(format!("{what} needs a number")))
    };
    let found: Vec<SeriesInfo> = if let Some(id) = term.strip_prefix("tmdb:") {
        vec![
            provider
                .series(number(id, "tmdb:")?)
                .await
                .map_err(metadata_error)?,
        ]
    } else if let Some(id) = term.strip_prefix("tvdb:") {
        vec![
            provider
                .series_by_tvdb_id(number(id, "tvdb:")?)
                .await
                .map_err(metadata_error)?,
        ]
    } else if let Some(id) = term.strip_prefix("imdb:") {
        vec![
            provider
                .series_by_imdb_id(id.trim())
                .await
                .map_err(metadata_error)?,
        ]
    } else {
        let (title, year) = split_year(term);
        provider
            .search_series(title, year)
            .await
            .map_err(metadata_error)?
    };
    let mut out = Vec::new();
    for s in &found {
        let existing = app
            .db
            .series()
            .find_by_tmdb_id(s.tmdb_id)
            .await?
            .map(|r| r.id);
        out.push(lookup_json(s, existing));
    }
    Ok(Json(Value::Array(out)))
}

// ---- adding and editing -----------------------------------------------------

fn parse_type(s: &str) -> Result<SeriesType, ApiError> {
    SeriesType::parse(s).ok_or_else(|| ApiError::bad_request(format!("unknown series_type '{s}'")))
}

fn parse_monitor(s: &str) -> Result<MonitorMode, ApiError> {
    serde_json::from_value(Value::String(s.to_owned()))
        .map_err(|_| ApiError::bad_request(format!("unknown monitor '{s}'")))
}

fn parse_new_items(s: &str) -> Result<NewItemMonitor, ApiError> {
    match s {
        "all" => Ok(NewItemMonitor::All),
        "none" => Ok(NewItemMonitor::None),
        other => Err(ApiError::bad_request(format!(
            "monitor_new_items must be all or none, not '{other}'"
        ))),
    }
}

#[derive(Deserialize)]
struct AddSeriesBody {
    tmdb_id: i64,
    quality_profile_id: i32,
    root_folder_path: String,
    series_type: Option<String>,
    #[serde(default = "default_monitor")]
    monitor: String,
    #[serde(default = "yes")]
    monitored: bool,
    #[serde(default = "yes")]
    season_folder: bool,
    #[serde(default = "default_new_items")]
    monitor_new_items: String,
    #[serde(default)]
    use_scene_numbering: bool,
    #[serde(default)]
    tags: Vec<i32>,
    /// Look for missing episodes as soon as the series is added.
    #[serde(default)]
    search_now: bool,
}

fn yes() -> bool {
    true
}

fn default_monitor() -> String {
    "all".into()
}

fn default_new_items() -> String {
    "all".into()
}

async fn add_series_handler(
    State(app): Shared,
    Json(req): Json<AddSeriesBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let Some(provider) = app.series_metadata.clone() else {
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
    app.db
        .quality_profiles()
        .get(req.quality_profile_id)
        .await
        .map_err(|_| ApiError::bad_request("unknown quality_profile_id"))?;
    let series_type = req.series_type.as_deref().map(parse_type).transpose()?;
    let monitor = parse_monitor(&req.monitor)?;
    let monitor_new_items = parse_new_items(&req.monitor_new_items)?;
    let search_now = req.search_now;

    if app
        .db
        .series()
        .find_by_tmdb_id(req.tmdb_id)
        .await?
        .is_some()
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "this series is already in the library".into(),
        ));
    }
    // The folder name comes from the series' title, so look it up first.
    let info = provider.series(req.tmdb_id).await.map_err(metadata_error)?;
    let naming = load_import_config(&app.db).await?.naming;
    let path = PathBuf::from(&req.root_folder_path)
        .join(build_series_folder(
            &NamingSeries {
                title: info.title.clone(),
                year: info.year,
                imdb_id: info.imdb_id.clone(),
                tvdb_id: info.tvdb_id,
                tmdb_id: info.tmdb_id,
                ..Default::default()
            },
            &naming,
        ))
        .to_string_lossy()
        .into_owned();

    let id = add_series(
        &app.db,
        &*PreFetched::new(provider, info),
        &AddSeries {
            tmdb_id: req.tmdb_id,
            quality_profile_id: req.quality_profile_id,
            path,
            root_folder_path: Some(req.root_folder_path),
            series_type,
            monitor,
            monitored: req.monitored,
            season_folder: req.season_folder,
            monitor_new_items,
            use_scene_numbering: req.use_scene_numbering,
            tags: req.tags,
        },
        Utc::now(),
    )
    .await?;
    // The folder may already hold episodes (an existing library).
    if let Err(e) = omp_services::rescan_series(&app.db, id, Utc::now()).await {
        tracing::warn!(series_id = id, "scanning the series folder failed: {e}");
    }
    if search_now {
        let app = app.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::tasks::search_missing_once(&app, Some(id), true).await {
                tracing::warn!(series_id = id, "search for the new series failed: {e}");
            }
        });
    }
    let s = app.db.series().get(id).await?;
    let episodes = app.db.episodes().for_series(id).await?;
    Ok((StatusCode::CREATED, Json(series_json(&s, &episodes, &[]))))
}

/// Lets `add_series` reuse the lookup the handler already made instead of
/// asking the provider a second time.
struct PreFetched {
    inner: Arc<dyn omp_metadata::SeriesMetadataProvider>,
    info: std::sync::Mutex<Option<SeriesInfo>>,
}

impl PreFetched {
    fn new(inner: Arc<dyn omp_metadata::SeriesMetadataProvider>, info: SeriesInfo) -> Box<Self> {
        Box::new(Self {
            inner,
            info: std::sync::Mutex::new(Some(info)),
        })
    }
}

#[async_trait::async_trait]
impl omp_metadata::SeriesMetadataProvider for PreFetched {
    async fn series(&self, tmdb_id: i64) -> Result<SeriesInfo, omp_metadata::MetadataError> {
        if let Some(info) = self.info.lock().unwrap().take()
            && info.tmdb_id == tmdb_id
        {
            return Ok(info);
        }
        self.inner.series(tmdb_id).await
    }
    async fn search_series(
        &self,
        term: &str,
        year: Option<i32>,
    ) -> Result<Vec<SeriesInfo>, omp_metadata::MetadataError> {
        self.inner.search_series(term, year).await
    }
    async fn series_by_imdb_id(
        &self,
        imdb_id: &str,
    ) -> Result<SeriesInfo, omp_metadata::MetadataError> {
        self.inner.series_by_imdb_id(imdb_id).await
    }
    async fn series_by_tvdb_id(
        &self,
        tvdb_id: i64,
    ) -> Result<SeriesInfo, omp_metadata::MetadataError> {
        self.inner.series_by_tvdb_id(tvdb_id).await
    }
}

#[derive(Deserialize)]
struct UpdateSeries {
    series_type: Option<String>,
    monitored: Option<bool>,
    monitor_new_items: Option<String>,
    quality_profile_id: Option<i32>,
    season_folder: Option<bool>,
    use_scene_numbering: Option<bool>,
    path: Option<String>,
    tags: Option<Vec<i32>>,
}

async fn update_series(
    State(app): Shared,
    Path(id): Path<i64>,
    Json(req): Json<UpdateSeries>,
) -> ApiResult {
    let current = app.db.series().get(id).await?;
    if let Some(p) = req.quality_profile_id {
        app.db
            .quality_profiles()
            .get(p)
            .await
            .map_err(|_| ApiError::bad_request("unknown quality_profile_id"))?;
    }
    app.db
        .series()
        .update(
            id,
            &SeriesUpdate {
                series_type: match req.series_type.as_deref() {
                    Some(t) => parse_type(t)?,
                    None => current.series_type,
                },
                monitored: req.monitored.unwrap_or(current.monitored),
                monitor_new_items: match req.monitor_new_items.as_deref() {
                    Some(m) => parse_new_items(m)?,
                    None => current.monitor_new_items,
                },
                quality_profile_id: req.quality_profile_id.unwrap_or(current.quality_profile_id),
                season_folder: req.season_folder.unwrap_or(current.season_folder),
                use_scene_numbering: req
                    .use_scene_numbering
                    .unwrap_or(current.use_scene_numbering),
                path: req.path.unwrap_or(current.path.clone()),
                root_folder_path: current.root_folder_path.clone(),
                tags: req.tags.unwrap_or(current.tags.clone()),
            },
        )
        .await?;
    get_series(State(app), Path(id)).await
}

#[derive(Deserialize, Default)]
struct DeleteSeries {
    /// Also delete the series' folder from disk (to the recycle bin if set up).
    #[serde(default)]
    delete_files: bool,
}

async fn delete_series(
    State(app): Shared,
    Path(id): Path<i64>,
    Query(q): Query<DeleteSeries>,
) -> ApiResult<StatusCode> {
    let s = app.db.series().get(id).await?;
    if q.delete_files && std::path::Path::new(&s.path).exists() {
        if !is_inside_a_root(&app, &s.path).await? {
            return Err(ApiError::bad_request(
                "the series folder is not inside a root folder, so it will not be deleted",
            ));
        }
        let config = load_import_config(&app.db).await?;
        let path = PathBuf::from(&s.path);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        omp_services::dispose_file(&config, &path, &name)
            .await
            .map_err(|e| ApiError::internal(format!("could not delete the folder: {e}")))?;
    }
    app.db.series().delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- monitoring --------------------------------------------------------------

#[derive(Deserialize)]
struct SeasonMonitor {
    monitored: bool,
}

/// Monitors or unmonitors a season and every episode in it.
async fn set_season_monitored(
    State(app): Shared,
    Path((id, season)): Path<(i64, i32)>,
    Json(req): Json<SeasonMonitor>,
) -> ApiResult<StatusCode> {
    app.db
        .series()
        .set_season_monitored(id, season, req.monitored)
        .await?;
    let ids: Vec<i64> = app
        .db
        .episodes()
        .for_series(id)
        .await?
        .into_iter()
        .filter(|e| e.season_number == season)
        .map(|e| e.id)
        .collect();
    app.db.episodes().set_monitored(&ids, req.monitored).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct EpisodeMonitor {
    episode_ids: Vec<i64>,
    monitored: bool,
}

async fn monitor_episodes(
    State(app): Shared,
    Json(req): Json<EpisodeMonitor>,
) -> ApiResult<StatusCode> {
    // Make sure every id is real before changing any.
    app.db.episodes().get_many(&req.episode_ids).await?;
    app.db
        .episodes()
        .set_monitored(&req.episode_ids, req.monitored)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ApplyMonitor {
    monitor: String,
}

async fn apply_monitor(
    State(app): Shared,
    Path(id): Path<i64>,
    Json(req): Json<ApplyMonitor>,
) -> ApiResult<StatusCode> {
    app.db.series().get(id).await?;
    apply_monitor_mode(&app.db, id, parse_monitor(&req.monitor)?, Utc::now()).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Deletes an episode file (to the recycle bin if set up); its episodes stay.
async fn delete_episode_file(State(app): Shared, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let file = app.db.episode_files().get(id).await?;
    let series = app.db.series().get(file.series_id).await?;
    let config = load_import_config(&app.db).await?;
    let path = std::path::Path::new(&series.path).join(&file.relative_path);
    if path.exists() {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        omp_services::dispose_file(&config, &path, &name)
            .await
            .map_err(|e| ApiError::internal(format!("could not delete the file: {e}")))?;
    }
    let episodes = app.db.episodes().for_file(id).await?;
    for e in &episodes {
        app.db
            .episode_history()
            .insert(&omp_db::repo::EpisodeHistoryRecord {
                id: 0,
                series_id: series.id,
                episode_id: e.id,
                event: omp_core::history::HistoryEvent::MovieFileDeleted,
                date: Utc::now(),
                source_title: file.relative_path.clone(),
                quality: file.quality,
                languages: file.languages.clone(),
                download_id: None,
                data: [("reason".to_owned(), "deleted by user".to_owned())].into(),
            })
            .await?;
    }
    app.db.episode_files().delete(id).await?;
    if config.unmonitor_deleted {
        let ids: Vec<i64> = episodes.iter().map(|e| e.id).collect();
        app.db.episodes().set_monitored(&ids, false).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Refreshes one series (or all) from the metadata provider.
pub(crate) async fn refresh(app: &App, only: Option<i64>) -> Result<usize, String> {
    let Some(provider) = app.series_metadata.clone() else {
        return Ok(0);
    };
    let ids: Vec<i64> = match only {
        Some(id) => vec![id],
        None => app
            .db
            .series()
            .list()
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|s| s.id)
            .collect(),
    };
    let mut refreshed = 0;
    for id in ids {
        match refresh_series(&app.db, &*provider, id).await {
            Ok(_) => {
                refreshed += 1;
                if let Err(e) = omp_services::rescan_series(&app.db, id, Utc::now()).await {
                    tracing::warn!(series_id = id, "scanning the series folder failed: {e}");
                }
            }
            Err(e) if only.is_some() => return Err(e.to_string()),
            Err(e) => tracing::warn!(series_id = id, "series refresh failed: {e}"),
        }
        if only.is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }
    Ok(refreshed)
}
