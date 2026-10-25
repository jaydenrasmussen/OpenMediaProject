//! Browsing TMDB: charts, titles with their cast and crew, people and the
//! companies behind a work. Everything is read from TMDB as it is asked for
//! and shaped so a page can show it; nothing is stored.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use omp_metadata::tmdb::TmdbProvider;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{ApiError, ApiResult, Shared, metadata_error},
    app::App,
};

const IMAGES: &str = "https://image.tmdb.org/t/p/original";

pub(crate) fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/browse/home", get(home))
        .route("/browse/list", get(list))
        .route("/browse/search", get(search))
        .route("/browse/title/{kind}/{id}", get(title))
        .route("/browse/person/{id}", get(person))
        .route("/browse/company/{id}", get(company))
}

fn provider(app: &App) -> Result<TmdbProvider, ApiError> {
    let Some(token) = app.config.tmdb_token.clone() else {
        return Err(ApiError::unavailable(
            "browsing needs TMDB (set TMDB_READ_TOKEN)",
        ));
    };
    let tmdb = TmdbProvider::new(token);
    Ok(match &app.config.tmdb_base_url {
        Some(base) => tmdb.with_base_url(base.clone()),
        None => tmdb,
    })
}

/// One provider for the life of the process, so its cache and pacing are shared.
fn shared(app: &App) -> Result<Arc<TmdbProvider>, ApiError> {
    static CELL: std::sync::Mutex<Option<(String, Arc<TmdbProvider>)>> =
        std::sync::Mutex::new(None);
    let key = format!("{:?}{:?}", app.config.tmdb_token, app.config.tmdb_base_url);
    let mut slot = CELL.lock().unwrap();
    if let Some((k, p)) = slot.as_ref()
        && *k == key
    {
        return Ok(p.clone());
    }
    let p = Arc::new(provider(app)?);
    *slot = Some((key, p.clone()));
    Ok(p)
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn picture(kind: &str, path: Option<String>) -> Option<Value> {
    path.map(|p| json!({ "kind": kind, "url": format!("{IMAGES}{p}") }))
}

fn year(v: &Value) -> i64 {
    text(v, "release_date")
        .or_else(|| text(v, "first_air_date"))
        .and_then(|d| d.get(..4).and_then(|y| y.parse().ok()))
        .unwrap_or(0)
}

/// A movie, series or person from a TMDB list, in the shape the pages use
/// (`images` as the library's covers are, so posters and banners work as they do there).
fn item(v: &Value, hint: &str) -> Option<Value> {
    let kind = text(v, "media_type").unwrap_or_else(|| hint.to_owned());
    if !matches!(kind.as_str(), "movie" | "tv" | "person") {
        return None;
    }
    let id = v.get("id")?.as_i64()?;
    let mut images = Vec::new();
    if kind == "person" {
        images.extend(picture("Poster", text(v, "profile_path")));
    } else {
        images.extend(picture("Poster", text(v, "poster_path")));
        images.extend(picture("Fanart", text(v, "backdrop_path")));
    }
    Some(json!({
        "kind": kind,
        "tmdb_id": id,
        "title": text(v, "title").or_else(|| text(v, "name")).unwrap_or_default(),
        "year": year(v),
        "overview": text(v, "overview"),
        "vote": v.get("vote_average").and_then(Value::as_f64).unwrap_or(0.0),
        "department": text(v, "known_for_department"),
        "role": text(v, "character").or_else(|| text(v, "job")),
        "images": images,
        "in_library": null,
    }))
}

/// Fills in `in_library` (the library id) and `media_servers` for every movie and series in the list.
async fn mark_library(app: &App, items: &mut [Value]) -> Result<(), ApiError> {
    let presence = app.presence();
    for it in items {
        let id = it["tmdb_id"].as_i64().unwrap_or(0);
        let title = it["title"].as_str().unwrap_or_default().to_owned();
        let year = it["year"].as_i64().unwrap_or(0);
        let (found, servers) = match it["kind"].as_str() {
            Some("movie") => (
                app.db
                    .movies()
                    .find_by_tmdb_id(id)
                    .await?
                    .map(|r| r.movie.id),
                presence.movie(Some(id), None, &title, year),
            ),
            Some("tv") => (
                app.db.series().find_by_tmdb_id(id).await?.map(|r| r.id),
                presence.series(Some(id), None, None, &title),
            ),
            _ => (None, Vec::new()),
        };
        it["in_library"] = json!(found);
        it["media_servers"] = json!(servers);
    }
    Ok(())
}

async fn items(app: &App, list: &Value, hint: &str) -> Result<Vec<Value>, ApiError> {
    let mut out: Vec<Value> = list
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| item(v, hint))
        .collect();
    mark_library(app, &mut out).await?;
    Ok(out)
}

async fn paged(app: &App, list: &Value, hint: &str) -> ApiResult {
    Ok(Json(json!({
        "page": list.get("page").and_then(Value::as_i64).unwrap_or(1),
        // TMDB serves no page past 500.
        "total_pages": list.get("total_pages").and_then(Value::as_i64).unwrap_or(1).min(500),
        "total_results": list.get("total_results").and_then(Value::as_i64).unwrap_or(0),
        "results": items(app, list, hint).await?,
    })))
}

const LISTS: &[(&str, &str, &str, &str)] = &[
    (
        "trending_movie",
        "/trending/movie/week",
        "movie",
        "Trending movies",
    ),
    ("popular_movie", "/movie/popular", "movie", "Popular movies"),
    ("top_movie", "/movie/top_rated", "movie", "Top rated movies"),
    ("upcoming_movie", "/movie/upcoming", "movie", "Coming soon"),
    ("trending_tv", "/trending/tv/week", "tv", "Trending series"),
    ("popular_tv", "/tv/popular", "tv", "Popular series"),
    ("top_tv", "/tv/top_rated", "tv", "Top rated series"),
];

/// The banner and the shelves of the Discover page.
async fn home(State(app): Shared) -> ApiResult {
    let tmdb = shared(&app)?;
    let day = tmdb
        .raw("/trending/all/day", &[])
        .await
        .map_err(metadata_error)?;
    // The first of today's trending with a backdrop and a story to tell.
    let hero = day
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|v| v.get("backdrop_path").is_some_and(|b| !b.is_null()))
        .filter(|v| text(v, "overview").is_some())
        .find_map(|v| item(v, "movie"))
        .filter(|v| v["kind"] != "person");
    let mut hero = hero;
    if let Some(h) = hero.as_mut() {
        mark_library(&app, std::slice::from_mut(h)).await?;
    }
    let mut rows = Vec::new();
    for (id, path, hint, label) in [LISTS[0], LISTS[4], LISTS[1], LISTS[5], LISTS[2], LISTS[3]] {
        // One chart being down leaves the others.
        let Ok(list) = tmdb.raw(path, &[]).await else {
            continue;
        };
        rows.push(json!({
            "id": id,
            "title": label,
            "items": items(&app, &list, hint).await?,
        }));
    }
    Ok(Json(json!({ "hero": hero, "rows": rows })))
}

#[derive(Deserialize)]
struct ListQuery {
    list: String,
    page: Option<i64>,
}

/// One chart, a page at a time.
async fn list(State(app): Shared, Query(q): Query<ListQuery>) -> ApiResult {
    let Some((_, path, hint, label)) = LISTS.iter().find(|l| l.0 == q.list) else {
        return Err(ApiError::bad_request("no such list"));
    };
    let tmdb = shared(&app)?;
    let page = q.page.unwrap_or(1).clamp(1, 500);
    let body = tmdb
        .raw(path, &[("page", page.to_string())])
        .await
        .map_err(metadata_error)?;
    let Json(mut out) = paged(&app, &body, hint).await?;
    out["title"] = json!(label);
    Ok(Json(out))
}

#[derive(Deserialize)]
struct SearchQuery {
    term: String,
    kind: Option<String>,
    page: Option<i64>,
}

/// Search, a page at a time. `kind` is movie, tv, person or (the default) all three.
async fn search(State(app): Shared, Query(q): Query<SearchQuery>) -> ApiResult {
    let term = q.term.trim();
    if term.is_empty() {
        return Err(ApiError::bad_request("term is required"));
    }
    let (path, hint) = match q.kind.as_deref().unwrap_or("multi") {
        "movie" => ("/search/movie", "movie"),
        "tv" => ("/search/tv", "tv"),
        "person" => ("/search/person", "person"),
        "multi" | "all" => ("/search/multi", "movie"),
        _ => return Err(ApiError::bad_request("kind is movie, tv, person or all")),
    };
    let (title, y) = crate::api::split_year(term);
    let mut query = vec![
        ("query", title.to_owned()),
        ("page", q.page.unwrap_or(1).clamp(1, 500).to_string()),
        ("include_adult", "false".to_owned()),
    ];
    if let Some(y) = y {
        match hint {
            "movie" if path == "/search/movie" => query.push(("year", y.to_string())),
            "tv" => query.push(("first_air_date_year", y.to_string())),
            _ => {}
        }
    }
    let body = shared(&app)?
        .raw(path, &query)
        .await
        .map_err(metadata_error)?;
    paged(&app, &body, hint).await
}

fn people(list: Option<&Vec<Value>>, jobs: &[&str]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for c in list.into_iter().flatten() {
        let job = text(c, "job").unwrap_or_default();
        if !jobs.contains(&job.as_str()) {
            continue;
        }
        let Some(id) = c.get("id").and_then(Value::as_i64) else {
            continue;
        };
        match out.iter_mut().find(|p| p["id"] == id) {
            Some(p) => {
                let jobs = p["job"].as_str().unwrap_or_default().to_owned();
                if !jobs.split(", ").any(|j| j == job) {
                    p["job"] = json!(format!("{jobs}, {job}"));
                }
            }
            None => out.push(json!({ "id": id, "name": text(c, "name"), "job": job })),
        }
    }
    out
}

/// A movie or series with its cast, crew, companies and what to watch next.
async fn title(State(app): Shared, Path((kind, id)): Path<(String, i64)>) -> ApiResult {
    if kind != "movie" && kind != "tv" {
        return Err(ApiError::bad_request("kind is movie or tv"));
    }
    let v = shared(&app)?
        .raw(
            &format!("/{kind}/{id}"),
            &[(
                "append_to_response",
                "credits,recommendations,videos".to_owned(),
            )],
        )
        .await
        .map_err(metadata_error)?;
    let mut base = item(&v, &kind).ok_or_else(|| ApiError::not_found("no such title"))?;
    mark_library(&app, std::slice::from_mut(&mut base)).await?;
    let cast: Vec<Value> = v["credits"]["cast"]
        .as_array()
        .into_iter()
        .flatten()
        .take(24)
        .filter_map(|c| {
            let mut p = item(c, "person")?;
            p["role"] = json!(text(c, "character"));
            Some(p)
        })
        .collect();
    let crew = v["credits"]["crew"].as_array();
    let mut directors = people(crew, &["Director"]);
    if let Some(made) = v.get("created_by").and_then(Value::as_array) {
        for c in made {
            if let Some(id) = c.get("id").and_then(Value::as_i64) {
                directors.push(json!({ "id": id, "name": text(c, "name"), "job": "Creator" }));
            }
        }
    }
    let companies: Vec<Value> = v["production_companies"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some(json!({
                "id": c.get("id")?.as_i64()?,
                "name": text(c, "name"),
                "logo": text(c, "logo_path").map(|p| format!("{IMAGES}{p}")),
                "country": text(c, "origin_country"),
            }))
        })
        .collect();
    let trailer = v["videos"]["results"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|t| t["site"] == "YouTube" && t["type"] == "Trailer")
        .and_then(|t| text(t, "key"));
    let recommended = items(&app, &v["recommendations"], &kind).await?;
    base["tagline"] = json!(text(&v, "tagline"));
    base["status"] = json!(text(&v, "status"));
    base["runtime"] = json!(
        v.get("runtime")
            .and_then(Value::as_i64)
            .unwrap_or_else(|| { v["episode_run_time"][0].as_i64().unwrap_or(0) })
    );
    base["genres"] = json!(
        v["genres"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| text(g, "name"))
            .collect::<Vec<_>>()
    );
    base["networks"] = json!(
        v["networks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| text(g, "name"))
            .collect::<Vec<_>>()
    );
    base["seasons"] = json!(v.get("number_of_seasons").and_then(Value::as_i64));
    base["cast"] = json!(cast);
    base["directors"] = json!(directors);
    base["producers"] = json!(people(crew, &["Producer", "Executive Producer"]));
    base["writers"] = json!(people(crew, &["Writer", "Screenplay", "Story"]));
    base["companies"] = json!(companies);
    base["trailer"] = json!(trailer);
    base["recommended"] = json!(recommended);
    Ok(Json(base))
}

/// A person: who they are and every work they appear in, by what they did on it.
async fn person(State(app): Shared, Path(id): Path<i64>) -> ApiResult {
    let v = shared(&app)?
        .raw(
            &format!("/person/{id}"),
            &[("append_to_response", "combined_credits".to_owned())],
        )
        .await
        .map_err(metadata_error)?;
    let credits = &v["combined_credits"];
    let mut sections = serde_json::Map::new();
    for (name, list, filter) in [
        ("Acting", "cast", None),
        ("Directing", "crew", Some(&["Director"][..])),
        (
            "Producing",
            "crew",
            Some(&["Producer", "Executive Producer"][..]),
        ),
        (
            "Writing",
            "crew",
            Some(&["Writer", "Screenplay", "Story", "Creator"][..]),
        ),
    ] {
        let mut rows: Vec<(String, Value)> = Vec::new();
        for c in credits[list].as_array().into_iter().flatten() {
            if let Some(jobs) = filter
                && !text(c, "job").is_some_and(|j| jobs.contains(&j.as_str()))
            {
                continue;
            }
            let Some(it) = item(c, "movie") else { continue };
            if rows
                .iter()
                .any(|(_, e)| e["kind"] == it["kind"] && e["tmdb_id"] == it["tmdb_id"])
            {
                continue;
            }
            let date = text(c, "release_date")
                .or_else(|| text(c, "first_air_date"))
                .unwrap_or_default();
            rows.push((date, it));
        }
        // Newest first; works with no date yet (announced) lead.
        rows.sort_by(|a, b| (b.0.is_empty(), &b.0).cmp(&(a.0.is_empty(), &a.0)));
        let mut list: Vec<Value> = rows.into_iter().map(|(_, v)| v).collect();
        mark_library(&app, &mut list).await?;
        if !list.is_empty() {
            sections.insert(name.to_owned(), json!(list));
        }
    }
    let mut me = item(&v, "person").ok_or_else(|| ApiError::not_found("no such person"))?;
    me["biography"] = json!(text(&v, "biography"));
    me["birthday"] = json!(text(&v, "birthday"));
    me["deathday"] = json!(text(&v, "deathday"));
    me["birthplace"] = json!(text(&v, "place_of_birth"));
    me["credits"] = Value::Object(sections);
    Ok(Json(me))
}

#[derive(Deserialize)]
struct CompanyQuery {
    kind: Option<String>,
    page: Option<i64>,
}

/// A company and the movies or series it made, a page at a time.
async fn company(
    State(app): Shared,
    Path(id): Path<i64>,
    Query(q): Query<CompanyQuery>,
) -> ApiResult {
    let kind = match q.kind.as_deref().unwrap_or("movie") {
        "tv" => "tv",
        "movie" => "movie",
        _ => return Err(ApiError::bad_request("kind is movie or tv")),
    };
    let tmdb = shared(&app)?;
    let info = tmdb
        .raw(&format!("/company/{id}"), &[])
        .await
        .map_err(metadata_error)?;
    let works = tmdb
        .raw(
            &format!("/discover/{kind}"),
            &[
                ("with_companies", id.to_string()),
                ("sort_by", "popularity.desc".to_owned()),
                ("page", q.page.unwrap_or(1).clamp(1, 500).to_string()),
            ],
        )
        .await
        .map_err(metadata_error)?;
    let Json(mut out) = paged(&app, &works, kind).await?;
    out["company"] = json!({
        "id": id,
        "name": text(&info, "name"),
        "description": text(&info, "description"),
        "logo": text(&info, "logo_path").map(|p| format!("{IMAGES}{p}")),
        "country": text(&info, "origin_country"),
        "homepage": text(&info, "homepage"),
        "headquarters": text(&info, "headquarters"),
    });
    Ok(Json(out))
}
