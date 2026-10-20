//! The TV side of searching, grabbing, history, blocklist, wanted and the calendar.

use std::{collections::HashMap, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get},
};
use chrono::{DateTime, Duration, Utc};
use omp_db::repo::{EpisodeRecord, SeriesRecord};
use omp_services::SearchMode;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{ApiError, ApiResult, Shared, event_name},
    app::App,
    tasks,
};

pub(crate) fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/release/episode", get(search_releases).post(grab_release))
        .route("/history/episode", get(history))
        .route(
            "/blocklist/series",
            get(list_blocklist).delete(clear_blocklist),
        )
        .route("/blocklist/series/{id}", delete(delete_blocklist_entry))
        .route("/wanted/episodes/missing", get(wanted_missing))
        .route("/wanted/episodes/cutoff", get(wanted_cutoff))
        .route("/calendar/episodes", get(calendar))
        .route("/rename/episodes", get(rename_previews))
}

fn paging(limit: Option<i64>, offset: Option<i64>) -> (i64, i64) {
    (
        limit.unwrap_or(50).clamp(1, 500),
        offset.unwrap_or(0).max(0),
    )
}

// ---- interactive search -------------------------------------------------------------

#[derive(Deserialize)]
struct EpisodeSearch {
    /// One episode...
    episode_id: Option<i64>,
    /// ...or a whole season.
    series_id: Option<i64>,
    season: Option<i32>,
}

async fn search_releases(State(app): Shared, Query(q): Query<EpisodeSearch>) -> ApiResult {
    let result = match (q.episode_id, q.series_id, q.season) {
        (Some(id), _, _) => {
            app.db.episodes().get(id).await?;
            tasks::search_episode_once(&app, id, SearchMode::Interactive, true).await?
        }
        (None, Some(series), Some(season)) => {
            app.db.series().get(series).await?;
            tasks::search_season_once(&app, series, season, SearchMode::Interactive, true).await?
        }
        _ => {
            return Err(ApiError::bad_request(
                "give an episode_id, or a series_id and a season",
            ));
        }
    };
    let failures: Vec<Value> = result
        .failures
        .iter()
        .map(|f| json!({ "indexer": f.indexer, "error": f.error.to_string() }))
        .collect();
    let mut rank: HashMap<usize, usize> = HashMap::new();
    for (n, i) in result
        .evaluation
        .approved_best_first()
        .into_iter()
        .enumerate()
    {
        rank.insert(i, n + 1);
    }
    let (search_id, evaluation) = app
        .tv_search_cache
        .lock()
        .unwrap()
        .insert(result.evaluation);
    let now = Utc::now();
    let mut items: Vec<(usize, Value)> = evaluation
        .items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let r = &it.release;
            let mut episodes: Vec<String> = it
                .episodes
                .iter()
                .map(|e| format!("{}x{:02}", e.season_number, e.episode_number))
                .collect();
            if it.parsed.as_ref().is_some_and(|p| p.full_season) {
                episodes = vec!["Season pack".into()];
            }
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
                    "matched_series_id": it.series.as_ref().map(|s| s.id),
                    "episodes": episodes,
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
    id: String,
}

async fn grab_release(State(app): Shared, Json(req): Json<GrabRelease>) -> ApiResult {
    let (search, index) = req
        .id
        .split_once('-')
        .and_then(|(s, i)| Some((s.parse::<u64>().ok()?, i.parse::<usize>().ok()?)))
        .ok_or_else(|| ApiError::bad_request("malformed release id"))?;
    let evaluation = app
        .tv_search_cache
        .lock()
        .unwrap()
        .get(search)
        .ok_or_else(|| ApiError::not_found("those search results have expired; search again"))?;
    if index >= evaluation.items.len() {
        return Err(ApiError::bad_request("no such release in those results"));
    }
    let ctx = omp_services::GrabContext {
        now: Utc::now(),
        process_start: app.started,
        category: Some(app.config.download_category.clone()),
    };
    let grabbed =
        omp_services::grab_selected_tv(&app.db, &evaluation, &app.registry(), &ctx, index)
            .await
            .map_err(|m| ApiError(StatusCode::BAD_GATEWAY, m))?;
    Ok(Json(json!({
        "download_id": grabbed.download_id,
        "client": grabbed.client_name,
        "title": evaluation.items[index].release.title,
    })))
}

// ---- history and blocklist ----------------------------------------------------------

#[derive(Deserialize)]
struct HistoryQuery {
    series_id: Option<i64>,
    episode_id: Option<i64>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn history(State(app): Shared, Query(q): Query<HistoryQuery>) -> ApiResult {
    let rows = match (q.episode_id, q.series_id) {
        (Some(e), _) => app.db.episode_history().for_episode(e).await?,
        (None, Some(s)) => app.db.episode_history().for_series(s).await?,
        _ => {
            let (limit, offset) = paging(q.limit, q.offset);
            app.db.episode_history().recent(limit, offset).await?
        }
    };
    let series: HashMap<i64, SeriesRecord> = app
        .db
        .series()
        .list()
        .await?
        .into_iter()
        .map(|s| (s.id, s))
        .collect();
    let mut episodes: HashMap<i64, EpisodeRecord> = HashMap::new();
    for h in &rows {
        if let std::collections::hash_map::Entry::Vacant(v) = episodes.entry(h.episode_id)
            && let Ok(e) = app.db.episodes().get(h.episode_id).await
        {
            v.insert(e);
        }
    }
    Ok(Json(Value::Array(
        rows.iter()
            .map(|h| {
                let e = episodes.get(&h.episode_id);
                json!({
                    "id": h.id,
                    "series_id": h.series_id,
                    "series_title": series.get(&h.series_id).map(|s| s.info.title.clone()),
                    "episode_id": h.episode_id,
                    "season_number": e.map(|e| e.season_number),
                    "episode_number": e.map(|e| e.episode_number),
                    "episode_title": e.map(|e| e.title.clone()),
                    "event": episode_event_name(h.event),
                    "date": h.date,
                    "source_title": h.source_title,
                    "quality": h.quality.quality.name,
                    "download_id": h.download_id,
                    "data": h.data,
                })
            })
            .collect(),
    )))
}

#[derive(Deserialize)]
struct BlocklistQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn list_blocklist(State(app): Shared, Query(q): Query<BlocklistQuery>) -> ApiResult {
    let (limit, offset) = paging(q.limit, q.offset);
    let rows = app.db.tv_blocklist().recent(limit, offset).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let b = &r.entry;
        let title = app
            .db
            .series()
            .get(b.movie_id)
            .await
            .map(|s| s.info.title)
            .unwrap_or_default();
        out.push(json!({
            "id": b.id,
            "series_id": b.movie_id,
            "series_title": title,
            "episode_ids": r.episode_ids,
            "source_title": b.source_title,
            "quality": b.quality.quality.name,
            "protocol": format!("{:?}", b.protocol),
            "indexer": b.indexer,
            "size": b.size,
            "message": b.message,
            "date": b.date,
        }));
    }
    Ok(Json(Value::Array(out)))
}

async fn delete_blocklist_entry(State(app): Shared, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    app.db.tv_blocklist().delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn clear_blocklist(State(app): Shared) -> ApiResult<StatusCode> {
    app.db.tv_blocklist().clear().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- wanted and calendar --------------------------------------------------------------

fn wanted_json(
    s: &SeriesRecord,
    e: &EpisodeRecord,
    file: Option<&omp_db::repo::EpisodeFileRecord>,
) -> Value {
    json!({
        "id": e.id,
        "series_id": s.id,
        "series_title": s.info.title,
        "season_number": e.season_number,
        "episode_number": e.episode_number,
        "title": e.title,
        "air_date": e.air_date,
        "air_date_utc": e.air_date_utc,
        "monitored": e.monitored,
        "last_search_time": e.last_search_time,
        "quality": file.map(|f| f.quality.quality.name),
    })
}

#[derive(Deserialize)]
struct WantedQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

fn page<T>(mut v: Vec<T>, q: &WantedQuery) -> Vec<T> {
    let (limit, offset) = paging(q.limit, q.offset);
    v.drain(..(offset as usize).min(v.len()));
    v.truncate(limit as usize);
    v
}

/// Monitored episodes that have aired and have no file, oldest first.
async fn wanted_missing(State(app): Shared, Query(q): Query<WantedQuery>) -> ApiResult {
    let now = Utc::now();
    let mut rows: Vec<(DateTime<Utc>, Value)> = Vec::new();
    for s in app.db.series().list().await? {
        if !s.monitored {
            continue;
        }
        for e in app.db.episodes().for_series(s.id).await? {
            if e.monitored && !e.has_file() && e.has_aired(now) {
                rows.push((e.air_date_utc.unwrap_or(now), wanted_json(&s, &e, None)));
            }
        }
    }
    rows.sort_by_key(|(d, _)| std::cmp::Reverse(*d));
    let total = rows.len();
    let items = page(rows.into_iter().map(|(_, v)| v).collect(), &q);
    Ok(Json(json!({ "total": total, "items": items })))
}

/// Monitored episodes whose file is below the profile's cutoff.
async fn wanted_cutoff(State(app): Shared, Query(q): Query<WantedQuery>) -> ApiResult {
    let profiles = app.db.quality_profiles().list().await?;
    let formats = app.db.custom_formats().load_compiled().await?;
    let mut rows: Vec<Value> = Vec::new();
    for s in app.db.series().list().await? {
        if !s.monitored {
            continue;
        }
        let Some(profile) = profiles.iter().find(|p| p.id == s.quality_profile_id) else {
            continue;
        };
        let files = app.db.episode_files().for_series(s.id).await?;
        for e in app.db.episodes().for_series(s.id).await? {
            let Some(file) = e
                .episode_file_id
                .and_then(|id| files.iter().find(|f| f.id == id))
            else {
                continue;
            };
            if !e.monitored {
                continue;
            }
            let ids = omp_services::matching_ids(
                &formats,
                &omp_services::input_for_episode_file(file, &s),
            );
            if omp_quality::upgrade::cutoff_not_met(profile, &file.quality, &ids, None) {
                rows.push(wanted_json(&s, &e, Some(file)));
            }
        }
    }
    let total = rows.len();
    Ok(Json(json!({ "total": total, "items": page(rows, &q) })))
}

#[derive(Deserialize)]
struct CalendarQuery {
    /// `YYYY-MM-DD`; defaults to today.
    start: Option<chrono::NaiveDate>,
    end: Option<chrono::NaiveDate>,
    #[serde(default)]
    unmonitored: bool,
}

async fn calendar(State(app): Shared, Query(q): Query<CalendarQuery>) -> ApiResult {
    let today = Utc::now().date_naive();
    let start = q.start.unwrap_or(today);
    let end = q.end.unwrap_or(start + Duration::days(30));
    let mut out = Vec::new();
    for s in app.db.series().list().await? {
        if !q.unmonitored && !s.monitored {
            continue;
        }
        let files = app.db.episode_files().for_series(s.id).await?;
        for e in app.db.episodes().for_series(s.id).await? {
            let Some(date) = e.air_date else { continue };
            if date < start || date > end || (!q.unmonitored && !e.monitored) {
                continue;
            }
            let file = e
                .episode_file_id
                .and_then(|id| files.iter().find(|f| f.id == id));
            out.push(wanted_json(&s, &e, file));
        }
    }
    out.sort_by(|a, b| {
        a["air_date_utc"]
            .as_str()
            .cmp(&b["air_date_utc"].as_str())
            .then(a["series_title"].as_str().cmp(&b["series_title"].as_str()))
    });
    Ok(Json(Value::Array(out)))
}

#[derive(Deserialize)]
struct RenameQuery {
    series_id: Option<i64>,
}

/// Files whose names differ from the current episode naming format.
async fn rename_previews(State(app): Shared, Query(q): Query<RenameQuery>) -> ApiResult {
    let naming = omp_services::load_import_config(&app.db).await?.naming;
    let previews = omp_services::episode_rename_previews(&app.db, &naming, q.series_id).await?;
    Ok(Json(Value::Array(
        previews
            .iter()
            .map(|p| {
                json!({
                    "series_id": p.series_id,
                    "series_title": p.series_title,
                    "episode_file_id": p.episode_file_id,
                    "episode_ids": p.episode_ids,
                    "existing": p.existing,
                    "new": p.new,
                })
            })
            .collect(),
    )))
}

fn episode_event_name(e: omp_core::history::HistoryEvent) -> &'static str {
    match event_name(e) {
        "movieFileDeleted" => "episodeFileDeleted",
        "movieFileRenamed" => "episodeFileRenamed",
        "movieFolderImported" => "seriesFolderImported",
        other => other,
    }
}
