//! The TV API end to end, against a fake metadata provider.

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use chrono::{Duration, NaiveDate, Utc};
use http_body_util::BodyExt;
use omp_app::{App, AppConfig, router};
use omp_core::{
    movie::{CoverKind, MediaCover},
    tv::{EpisodeInfo, SeasonInfo, SeriesInfo, SeriesStatus},
};
use omp_db::Db;
use omp_metadata::{MetadataError, SeriesMetadataProvider};
use omp_services::{MediaProbe, NoDiskCheck, ProbeInfo};
use serde_json::{Value, json};
use tower::ServiceExt;

struct NoProbe;

#[async_trait]
impl MediaProbe for NoProbe {
    async fn probe(&self, _: &Path) -> Option<ProbeInfo> {
        None
    }
}

struct FakeTv {
    series: Mutex<SeriesInfo>,
}

fn episode(season: i32, number: i32, days: i64) -> EpisodeInfo {
    let when = Utc::now() + Duration::days(days);
    EpisodeInfo {
        tmdb_id: i64::from(season * 100 + number),
        season_number: season,
        episode_number: number,
        title: format!("Episode {season}x{number}"),
        air_date: Some(when.date_naive()),
        air_date_utc: Some(when),
        runtime: 45,
        ..Default::default()
    }
}

fn show() -> SeriesInfo {
    SeriesInfo {
        tmdb_id: 1396,
        tvdb_id: Some(81189),
        imdb_id: Some("tt0903747".into()),
        title: "Breaking Bad".into(),
        original_title: "Breaking Bad".into(),
        original_language: "en".into(),
        overview: Some("A teacher.".into()),
        year: 2008,
        status: SeriesStatus::Continuing,
        network: Some("AMC".into()),
        runtime: 47,
        genres: vec!["Drama".into()],
        first_aired: NaiveDate::from_ymd_opt(2008, 1, 20),
        images: vec![MediaCover {
            kind: CoverKind::Poster,
            url: "https://image.example/p.jpg".into(),
        }],
        seasons: vec![
            SeasonInfo {
                season_number: 0,
                title: Some("Specials".into()),
                episode_count: 1,
                images: vec![],
            },
            SeasonInfo {
                season_number: 1,
                title: None,
                episode_count: 2,
                images: vec![],
            },
            SeasonInfo {
                season_number: 2,
                title: None,
                episode_count: 1,
                images: vec![],
            },
        ],
        episodes: vec![
            episode(0, 1, -500),
            episode(1, 1, -300),
            episode(1, 2, -293),
            episode(2, 1, 30),
        ],
        ..Default::default()
    }
}

#[async_trait]
impl SeriesMetadataProvider for FakeTv {
    async fn series(&self, tmdb_id: i64) -> Result<SeriesInfo, MetadataError> {
        let s = self.series.lock().unwrap().clone();
        if s.tmdb_id == tmdb_id {
            Ok(s)
        } else {
            Err(MetadataError::NotFound)
        }
    }
    async fn search_series(
        &self,
        term: &str,
        _: Option<i32>,
    ) -> Result<Vec<SeriesInfo>, MetadataError> {
        if term.to_lowercase().contains("breaking") {
            let mut s = self.series.lock().unwrap().clone();
            s.episodes.clear();
            s.seasons.clear();
            Ok(vec![s])
        } else {
            Ok(vec![])
        }
    }
    async fn series_by_imdb_id(&self, imdb: &str) -> Result<SeriesInfo, MetadataError> {
        if imdb == "tt0903747" {
            self.series(1396).await
        } else {
            Err(MetadataError::NotFound)
        }
    }
    async fn series_by_tvdb_id(&self, tvdb: i64) -> Result<SeriesInfo, MetadataError> {
        if tvdb == 81189 {
            self.series(1396).await
        } else {
            Err(MetadataError::NotFound)
        }
    }
}

struct Rig {
    router: Router,
    app: Arc<App>,
    provider: Arc<FakeTv>,
    key: String,
    dir: tempfile::TempDir,
}

async fn rig(with_provider: bool) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(FakeTv {
        series: Mutex::new(show()),
    });
    let app = App::with_series_metadata(
        AppConfig::new(dir.path().join("data")),
        Db::in_memory().await.unwrap(),
        None,
        with_provider.then(|| provider.clone() as Arc<dyn SeriesMetadataProvider>),
        Arc::new(NoProbe),
        Arc::new(NoDiskCheck),
    )
    .await
    .unwrap();
    let key = app.api_key().await.unwrap();
    Rig {
        router: router(app.clone()),
        app,
        provider,
        key,
        dir,
    }
}

async fn call(r: &Rig, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("x-api-key", &r.key);
    let body = match body {
        Some(b) => {
            req = req.header("content-type", "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let res = r
        .router
        .clone()
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn get(r: &Rig, path: &str) -> (StatusCode, Value) {
    call(r, Method::GET, path, None).await
}

async fn add_root(r: &Rig) -> String {
    let path = r.dir.path().join("tv");
    std::fs::create_dir_all(&path).unwrap();
    let path = path.to_string_lossy().into_owned();
    let (s, _) = call(
        r,
        Method::POST,
        "/api/v1/rootfolder",
        Some(json!({ "path": path })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    path
}

async fn profile(r: &Rig) -> i64 {
    get(r, "/api/v1/qualityprofile").await.1[0]["id"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn lookup_finds_series_by_title_or_id_and_marks_those_in_the_library() {
    let r = rig(true).await;
    let (s, hits) = get(&r, "/api/v1/series/lookup?term=breaking%20bad").await;
    assert_eq!(s, StatusCode::OK, "{hits}");
    assert_eq!(hits[0]["tmdb_id"], 1396);
    assert_eq!(hits[0]["in_library"], Value::Null);
    for term in ["tmdb:1396", "tvdb:81189", "imdb:tt0903747"] {
        let (s, one) = get(&r, &format!("/api/v1/series/lookup?term={term}")).await;
        assert_eq!(
            (s, one[0]["title"].clone()),
            (StatusCode::OK, json!("Breaking Bad")),
            "{term}"
        );
    }
    assert_eq!(
        get(&r, "/api/v1/series/lookup?term=tmdb:abc").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get(&r, "/api/v1/series/lookup?term=").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get(&r, "/api/v1/series/lookup?term=tmdb:5").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&r, "/api/v1/series/lookup?term=nothing%20like%20it")
            .await
            .1,
        json!([])
    );

    let bare = rig(false).await;
    assert_eq!(
        get(&bare, "/api/v1/series/lookup?term=x").await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn adding_validates_then_stores_the_series_with_its_episodes() {
    let r = rig(true).await;
    let root = add_root(&r).await;
    let p = profile(&r).await;
    let add = |body: Value| call(&r, Method::POST, "/api/v1/series", Some(body));
    let base = json!({ "tmdb_id": 1396, "quality_profile_id": p, "root_folder_path": root });

    for (change, why) in [
        (json!({ "root_folder_path": "/nowhere" }), "root"),
        (json!({ "quality_profile_id": 9999 }), "profile"),
        (json!({ "series_type": "weird" }), "type"),
        (json!({ "monitor": "sometimes" }), "monitor"),
        (json!({ "monitor_new_items": "maybe" }), "new items"),
    ] {
        let mut body = base.clone();
        for (k, v) in change.as_object().unwrap() {
            body[k] = v.clone();
        }
        assert_eq!(add(body).await.0, StatusCode::BAD_REQUEST, "{why}");
    }
    let mut unknown = base.clone();
    unknown["tmdb_id"] = json!(5);
    assert_eq!(add(unknown).await.0, StatusCode::NOT_FOUND);

    let mut body = base.clone();
    body["monitor"] = json!("future");
    let (s, series) = add(body).await;
    assert_eq!(s, StatusCode::CREATED, "{series}");
    assert_eq!(series["title"], "Breaking Bad");
    assert_eq!(series["path"], format!("{root}/Breaking Bad (2008)"));
    assert_eq!(series["total_episode_count"], 4);
    assert_eq!(
        series["episode_count"], 1,
        "only what is monitored (or has a file) counts toward complete"
    );
    assert_eq!(series["monitored_episode_count"], 1, "future only");
    assert_eq!(series["series_type"], "standard");
    let id = series["id"].as_i64().unwrap();

    // The same series cannot be added twice.
    assert_eq!(add(base).await.0, StatusCode::CONFLICT);

    let (_, list) = get(&r, "/api/v1/series").await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    let (_, one) = get(&r, &format!("/api/v1/series/{id}")).await;
    let seasons: Vec<_> = one["seasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["season_number"].as_i64().unwrap(),
                s["monitored"].as_bool().unwrap(),
                s["total_episode_count"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(seasons, [(0, false, 1), (1, false, 2), (2, true, 1)]);
    assert!(one["next_airing"].is_string());

    let (_, hit) = get(&r, "/api/v1/series/lookup?term=tmdb:1396").await;
    assert_eq!(hit[0]["in_library"], id);
}

#[tokio::test]
async fn episodes_and_seasons_can_be_monitored_and_listed() {
    let r = rig(true).await;
    let root = add_root(&r).await;
    let p = profile(&r).await;
    let (_, series) = call(
        &r,
        Method::POST,
        "/api/v1/series",
        Some(json!({
            "tmdb_id": 1396, "quality_profile_id": p, "root_folder_path": root, "monitor": "none",
        })),
    )
    .await;
    let id = series["id"].as_i64().unwrap();

    let (_, all) = get(&r, &format!("/api/v1/episode?series_id={id}")).await;
    assert_eq!(all.as_array().unwrap().len(), 4);
    assert!(
        all.as_array()
            .unwrap()
            .iter()
            .all(|e| e["monitored"] == false)
    );
    let (_, s1) = get(&r, &format!("/api/v1/episode?series_id={id}&season=1")).await;
    assert_eq!(s1.as_array().unwrap().len(), 2);
    let ep_ids: Vec<i64> = s1
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_i64().unwrap())
        .collect();

    let (s, _) = call(
        &r,
        Method::PUT,
        "/api/v1/episode/monitor",
        Some(json!({ "episode_ids": [ep_ids[0]], "monitored": true })),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert_eq!(
        get(&r, &format!("/api/v1/episode/{}", ep_ids[0])).await.1["monitored"],
        true
    );
    assert_eq!(
        call(
            &r,
            Method::PUT,
            "/api/v1/episode/monitor",
            Some(json!({ "episode_ids": [ep_ids[0], 9999], "monitored": false }))
        )
        .await
        .0,
        StatusCode::NOT_FOUND,
        "nothing changes when one id is bad"
    );
    assert_eq!(
        get(&r, &format!("/api/v1/episode/{}", ep_ids[0])).await.1["monitored"],
        true
    );

    // A whole season at once.
    call(
        &r,
        Method::PUT,
        &format!("/api/v1/series/{id}/season/2"),
        Some(json!({ "monitored": true })),
    )
    .await;
    let (_, one) = get(&r, &format!("/api/v1/series/{id}")).await;
    assert_eq!(one["monitored_episode_count"], 2);
    assert_eq!(
        call(
            &r,
            Method::PUT,
            &format!("/api/v1/series/{id}/season/9"),
            Some(json!({ "monitored": true }))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    // A monitor mode can be applied again later.
    let (s, _) = call(
        &r,
        Method::POST,
        &format!("/api/v1/series/{id}/monitor"),
        Some(json!({ "monitor": "all" })),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert_eq!(
        get(&r, &format!("/api/v1/series/{id}")).await.1["monitored_episode_count"],
        3
    );
    assert_eq!(
        call(
            &r,
            Method::POST,
            &format!("/api/v1/series/{id}/monitor"),
            Some(json!({ "monitor": "bogus" }))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn editing_a_series_and_refreshing_it() {
    let r = rig(true).await;
    let root = add_root(&r).await;
    let p = profile(&r).await;
    let (_, series) = call(
        &r,
        Method::POST,
        "/api/v1/series",
        Some(json!({
            "tmdb_id": 1396, "quality_profile_id": p, "root_folder_path": root,
        })),
    )
    .await;
    let id = series["id"].as_i64().unwrap();

    let (s, edited) = call(
        &r,
        Method::PUT,
        &format!("/api/v1/series/{id}"),
        Some(json!({
            "series_type": "anime", "monitored": false, "season_folder": false, "tags": [],
        })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{edited}");
    assert_eq!(
        (
            edited["series_type"].clone(),
            edited["monitored"].clone(),
            edited["season_folder"].clone()
        ),
        (json!("anime"), json!(false), json!(false))
    );
    assert_eq!(
        call(
            &r,
            Method::PUT,
            &format!("/api/v1/series/{id}"),
            Some(json!({ "quality_profile_id": 9999 }))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    // The provider moves on.
    let mut changed = show();
    changed.title = "Breaking Bad (Remastered)".into();
    changed.episodes.push(episode(2, 2, 37));
    *r.provider.series.lock().unwrap() = changed;
    let (s, _) = call(
        &r,
        Method::POST,
        "/api/v1/command",
        Some(json!({ "name": "RefreshSeries", "series_id": id, "wait": true })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, after) = get(&r, &format!("/api/v1/series/{id}")).await;
    assert_eq!(after["title"], "Breaking Bad (Remastered)");
    assert_eq!(after["total_episode_count"], 5);
    assert_eq!(after["series_type"], "anime", "the choice survives");

    assert_eq!(
        call(
            &r,
            Method::POST,
            "/api/v1/command",
            Some(json!({ "name": "RefreshSeries" }))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (s, _) = call(
        &r,
        Method::POST,
        "/api/v1/command",
        Some(json!({ "name": "RefreshAllSeries", "wait": true })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn removing_a_series_never_deletes_outside_the_root_folders() {
    let r = rig(true).await;
    let root = add_root(&r).await;
    let p = profile(&r).await;
    let (_, series) = call(
        &r,
        Method::POST,
        "/api/v1/series",
        Some(json!({
            "tmdb_id": 1396, "quality_profile_id": p, "root_folder_path": root,
        })),
    )
    .await;
    let id = series["id"].as_i64().unwrap();
    let folder = std::path::PathBuf::from(series["path"].as_str().unwrap());
    std::fs::create_dir_all(folder.join("Season 01")).unwrap();
    std::fs::write(folder.join("Season 01/a.mkv"), b"x").unwrap();

    // Pointed somewhere precious, the files are refused.
    let precious = r.dir.path().join("precious");
    std::fs::create_dir_all(&precious).unwrap();
    call(
        &r,
        Method::PUT,
        &format!("/api/v1/series/{id}"),
        Some(json!({ "path": precious.to_string_lossy() })),
    )
    .await;
    assert_eq!(
        call(
            &r,
            Method::DELETE,
            &format!("/api/v1/series/{id}?delete_files=true"),
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert!(precious.is_dir());

    call(
        &r,
        Method::PUT,
        &format!("/api/v1/series/{id}"),
        Some(json!({ "path": folder.to_string_lossy() })),
    )
    .await;
    assert_eq!(
        call(
            &r,
            Method::DELETE,
            &format!("/api/v1/series/{id}?delete_files=true"),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert!(!folder.exists());
    assert_eq!(
        get(&r, &format!("/api/v1/series/{id}")).await.0,
        StatusCode::NOT_FOUND
    );
    assert!(r.app.db.episodes().for_series(id).await.unwrap().is_empty());
}

#[tokio::test]
async fn wanted_calendar_history_and_blocklist_cover_episodes() {
    let r = rig(true).await;
    let root = add_root(&r).await;
    let p = profile(&r).await;
    let (s, series) = call(
        &r,
        Method::POST,
        "/api/v1/series",
        Some(json!({ "tmdb_id": 1396, "quality_profile_id": p, "root_folder_path": root, "monitor": "all" })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{series}");
    let id = series["id"].as_i64().unwrap();

    // Two aired, monitored episodes have no file (the special and the future one don't count).
    let (_, missing) = get(&r, "/api/v1/wanted/episodes/missing").await;
    assert!(missing["total"].as_i64().unwrap() >= 2, "{missing}");
    assert_eq!(missing["items"][0]["series_title"], "Breaking Bad");
    let (_, paged) = get(&r, "/api/v1/wanted/episodes/missing?limit=1").await;
    assert_eq!(paged["items"].as_array().unwrap().len(), 1);
    let (_, cutoff) = get(&r, "/api/v1/wanted/episodes/cutoff").await;
    assert_eq!(cutoff["total"], 0);

    // The future episode appears on the calendar.
    let start = Utc::now().date_naive();
    let end = start + Duration::days(60);
    let (s, cal) = get(
        &r,
        &format!("/api/v1/calendar/episodes?start={start}&end={end}"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(cal.as_array().unwrap().len(), 1, "{cal}");
    assert_eq!(cal[0]["season_number"], 2);

    assert_eq!(get(&r, "/api/v1/history/episode").await.1, json!([]));
    assert_eq!(get(&r, "/api/v1/blocklist/series").await.1, json!([]));
    assert_eq!(
        call(&r, Method::DELETE, "/api/v1/blocklist/series/9", None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    // Searches need to know what to look for.
    assert_eq!(
        get(&r, "/api/v1/release/episode").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get(&r, "/api/v1/release/episode?episode_id=999").await.0,
        StatusCode::NOT_FOUND
    );
    for (cmd, ok) in [
        (json!({ "name": "EpisodeSearch" }), false),
        (json!({ "name": "SeasonSearch", "series_id": id }), false),
        (json!({ "name": "SeriesSearch" }), false),
        (
            json!({ "name": "SeriesSearch", "series_id": id, "wait": true }),
            true,
        ),
        (
            json!({ "name": "MissingEpisodesSearch", "wait": true }),
            true,
        ),
    ] {
        let (s, body) = call(&r, Method::POST, "/api/v1/command", Some(cmd.clone())).await;
        assert_eq!(s.is_success(), ok, "{cmd}: {s} {body}");
    }
}

#[tokio::test]
async fn existing_series_folders_can_be_adopted_with_their_files() {
    let r = rig(true).await;
    let root = add_root(&r).await;
    let p = profile(&r).await;
    let folder = std::path::Path::new(&root).join("Breaking Bad (2008)");
    std::fs::create_dir_all(folder.join("Season 1")).unwrap();
    std::fs::write(
        folder.join("Season 1/Breaking.Bad.S01E01.720p.BluRay.x264-GRP.mkv"),
        b"x",
    )
    .unwrap();
    std::fs::create_dir_all(std::path::Path::new(&root).join("Empty Show")).unwrap();

    let (_, roots) = get(&r, "/api/v1/rootfolder").await;
    let root_id = roots[0]["id"].as_i64().unwrap();
    let (s, folders) = get(&r, &format!("/api/v1/rootfolder/{root_id}/unmapped-series")).await;
    assert_eq!(s, StatusCode::OK, "{folders}");
    let names: Vec<_> = folders
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Breaking Bad (2008)", "Empty Show"]);
    assert_eq!(folders[0]["has_media"], true);

    let body = json!({
        "items": [
            { "path": folder.to_string_lossy(), "tmdb_id": 1396 },
            { "path": "/elsewhere/Show", "tmdb_id": 1396 },
        ],
        "quality_profile_id": p,
    });
    let (s, res) = call(
        &r,
        Method::POST,
        "/api/v1/series/import",
        Some(body.clone()),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{res}");
    assert_eq!(res["results"][0]["ok"], true, "{res}");
    assert_eq!(res["results"][1]["ok"], false);
    let id = res["results"][0]["series_id"].as_i64().unwrap();

    let (_, eps) = get(&r, &format!("/api/v1/episode?series_id={id}")).await;
    let with_file: Vec<_> = eps
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| !e["file"].is_null())
        .collect();
    assert_eq!(with_file.len(), 1, "{eps}");
    assert_eq!(with_file[0]["episode_number"], 1);

    // It is in the library now, so the folder is no longer offered or importable.
    let (_, folders) = get(&r, &format!("/api/v1/rootfolder/{root_id}/unmapped-series")).await;
    assert_eq!(folders.as_array().unwrap().len(), 1);
    let (_, again) = call(&r, Method::POST, "/api/v1/series/import", Some(body)).await;
    assert_eq!(again["results"][0]["ok"], false);
}
