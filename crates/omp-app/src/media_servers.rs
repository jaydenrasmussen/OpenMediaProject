//! Plex and Jellyfin: reading what their libraries hold, so the app can show
//! what is already there. Read-only; nothing is ever written to a server.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use omp_db::repo::{MediaItem, Provider, ProviderKind, SyncStatus};
use omp_plugin_host::{MediaItemKind, MediaLibraryItem};
use serde_json::{Value, json};

use crate::app::App;

/// More than this from one server is a mistake, not a library.
const MAX_ITEMS: usize = 1_000_000;
const MAX_PAGES: usize = 20_000;

/// Reads a server's whole library, a page at a time.
pub async fn read_library(app: &App, p: &Provider) -> Result<Vec<MediaItem>, String> {
    let plugin_id = p.plugin_id.as_deref().ok_or("no plugin chosen")?;
    let plugin = app
        .plugins()
        .media_server(plugin_id)
        .ok_or_else(|| format!("plugin '{plugin_id}' isn't installed"))?;
    let settings = p.settings.to_string();
    let mut items = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let page = plugin
            .library_page(cursor.as_deref(), &settings)
            .await
            .map_err(|e| format!("{e:#}"))?;
        items.extend(page.items.into_iter().map(item_row));
        if items.len() > MAX_ITEMS {
            return Err(format!("more than {MAX_ITEMS} items; giving up"));
        }
        match page.next {
            Some(next) if cursor.as_deref() != Some(next.as_str()) => cursor = Some(next),
            // No next page, or the plugin asked for the page it just gave.
            _ => return Ok(items),
        }
    }
    Err("the server has more pages than can be read".into())
}

fn item_row(i: MediaLibraryItem) -> MediaItem {
    MediaItem {
        key: i.key,
        kind: match i.kind {
            MediaItemKind::Movie => "movie",
            MediaItemKind::Series => "series",
            MediaItemKind::Episode => "episode",
        }
        .into(),
        series_key: i.series_key,
        title: i.title,
        year: i.year.map(i64::from),
        tmdb_id: i.tmdb_id.filter(|id| *id > 0),
        imdb_id: i.imdb_id.filter(|s| !s.is_empty()),
        tvdb_id: i.tvdb_id.filter(|id| *id > 0),
        season: i.season.map(i64::from),
        episode: i.episode.map(i64::from),
    }
}

/// Reads one server and stores what it holds. A failure is recorded and
/// leaves the last good copy in place.
pub async fn sync_provider(app: &App, p: &Provider) -> Result<SyncStatus, String> {
    let items = read_library(app, p).await;
    let mut status = SyncStatus {
        provider_id: p.id,
        synced_at: Some(Utc::now().to_rfc3339()),
        ..Default::default()
    };
    match &items {
        Ok(items) => {
            let count = |kind: &str| items.iter().filter(|i| i.kind == kind).count() as i64;
            status.movies = count("movie");
            status.series = count("series");
            status.episodes = count("episode");
            app.db
                .media_server_items()
                .replace(p.id, items)
                .await
                .map_err(|e| e.to_string())?;
        }
        Err(e) => {
            // Keep the counts of the last good sync beside the error.
            if let Ok(all) = app.db.media_server_items().statuses().await
                && let Some(old) = all.into_iter().find(|s| s.provider_id == p.id)
            {
                status.movies = old.movies;
                status.series = old.series;
                status.episodes = old.episodes;
                status.synced_at = old.synced_at;
            }
            status.error = Some(e.clone());
        }
    }
    app.db
        .media_server_items()
        .set_status(&status)
        .await
        .map_err(|e| e.to_string())?;
    items.map(|_| status)
}

/// Reads every enabled server, then refreshes what the pages show.
pub async fn sync_all(app: &App) -> Result<String, String> {
    let servers = app
        .db
        .providers()
        .list(ProviderKind::MediaServer)
        .await
        .map_err(|e| e.to_string())?;
    let mut problems = Vec::new();
    let mut synced = 0;
    for p in servers.iter().filter(|p| p.enabled) {
        match sync_provider(app, p).await {
            Ok(_) => synced += 1,
            Err(e) => problems.push(format!("{}: {e}", p.name)),
        }
    }
    app.refresh_presence().await;
    if problems.is_empty() {
        Ok(format!("{synced} servers read"))
    } else {
        Err(problems.join("; "))
    }
}

/// What the media servers hold that this library does not: movies and series
/// with an id the library lacks, with the servers they are on.
pub async fn untracked(app: &App) -> Result<Value, String> {
    let e = |e: omp_db::DbError| e.to_string();
    let movies = app.db.movies().list().await.map_err(e)?;
    let series = app.db.series().list().await.map_err(e)?;
    let have_movie: HashSet<i64> = movies.iter().map(|m| m.movie.tmdb_id).collect();
    let have_series: HashSet<i64> = series.iter().map(|s| s.info.tmdb_id).collect();
    let names: HashMap<i64, String> = app
        .db
        .providers()
        .list(ProviderKind::MediaServer)
        .await
        .map_err(e)?
        .into_iter()
        .filter(|p| p.enabled)
        .map(|p| (p.id, p.name))
        .collect();
    // One entry per title, listing every server that has it.
    let mut found: HashMap<(String, i64), (MediaItem, Vec<String>)> = HashMap::new();
    for (server, i) in app.db.media_server_items().all().await.map_err(e)? {
        let Some(name) = names.get(&server) else {
            continue;
        };
        let held = match i.kind.as_str() {
            "movie" => i.tmdb_id.is_some_and(|id| have_movie.contains(&id)),
            "series" => i.tmdb_id.is_some_and(|id| have_series.contains(&id)),
            _ => continue,
        };
        // Without a TMDB id the entry cannot be added from here; leave it out.
        if held || i.tmdb_id.is_none() {
            continue;
        }
        let key = (i.kind.clone(), i.tmdb_id.unwrap_or_default());
        let slot = found.entry(key).or_insert_with(|| (i, Vec::new()));
        if !slot.1.contains(name) {
            slot.1.push(name.clone());
        }
    }
    let mut out = (Vec::new(), Vec::new());
    for ((kind, tmdb), (i, servers)) in found {
        let v = json!({ "tmdb_id": tmdb, "title": i.title, "year": i.year, "servers": servers });
        if kind == "movie" {
            out.0.push(v)
        } else {
            out.1.push(v)
        }
    }
    let by_title = |a: &Value, b: &Value| a["title"].as_str().cmp(&b["title"].as_str());
    out.0.sort_by(by_title);
    out.1.sort_by(by_title);
    Ok(json!({ "movies": out.0, "series": out.1 }))
}

/// Lower case, letters and digits only, so "Spider-Man: Homecoming" and
/// "Spider Man Homecoming" meet.
pub fn normalize(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// What the servers hold, indexed for quick answers. Rebuilt after each sync.
#[derive(Default)]
pub struct Presence {
    names: HashMap<i64, String>,
    movies_tmdb: HashMap<i64, Vec<i64>>,
    movies_imdb: HashMap<String, Vec<i64>>,
    movies_title: HashMap<(String, i64), Vec<i64>>,
    series: Vec<SeriesEntry>,
    /// (server, series key) to the episodes it has.
    episodes: HashMap<(i64, String), HashSet<(i64, i64)>>,
}

struct SeriesEntry {
    server: i64,
    key: String,
    tmdb: Option<i64>,
    tvdb: Option<i64>,
    imdb: Option<String>,
    title: String,
}

impl Presence {
    pub async fn build(app: &App) -> Result<Self, omp_db::DbError> {
        let names = app
            .db
            .providers()
            .list(ProviderKind::MediaServer)
            .await?
            .into_iter()
            .filter(|p| p.enabled)
            .map(|p| (p.id, p.name))
            .collect::<HashMap<_, _>>();
        let mut me = Presence::default();
        for (server, i) in app.db.media_server_items().all().await? {
            if !names.contains_key(&server) {
                continue;
            }
            match i.kind.as_str() {
                "movie" => {
                    if let Some(id) = i.tmdb_id {
                        me.movies_tmdb.entry(id).or_default().push(server);
                    }
                    if let Some(id) = &i.imdb_id {
                        me.movies_imdb.entry(id.clone()).or_default().push(server);
                    }
                    if let Some(y) = i.year {
                        me.movies_title
                            .entry((normalize(&i.title), y))
                            .or_default()
                            .push(server);
                    }
                }
                "series" => me.series.push(SeriesEntry {
                    server,
                    key: i.key,
                    tmdb: i.tmdb_id,
                    tvdb: i.tvdb_id,
                    imdb: i.imdb_id,
                    title: normalize(&i.title),
                }),
                _ => {
                    if let (Some(sk), Some(s), Some(e)) = (i.series_key, i.season, i.episode) {
                        me.episodes.entry((server, sk)).or_default().insert((s, e));
                    }
                }
            }
        }
        me.names = names;
        Ok(me)
    }

    fn server_names(&self, ids: impl IntoIterator<Item = i64>) -> Vec<String> {
        let mut out: Vec<String> = ids
            .into_iter()
            .collect::<HashSet<_>>()
            .into_iter()
            .filter_map(|id| self.names.get(&id).cloned())
            .collect();
        out.sort();
        out
    }

    /// The servers that hold a movie: by TMDB id, else IMDb id, else title and year.
    pub fn movie(
        &self,
        tmdb: Option<i64>,
        imdb: Option<&str>,
        title: &str,
        year: i64,
    ) -> Vec<String> {
        let mut ids: Vec<i64> = Vec::new();
        if let Some(v) = tmdb.and_then(|t| self.movies_tmdb.get(&t)) {
            ids.extend(v);
        }
        if let Some(v) = imdb.and_then(|i| self.movies_imdb.get(i)) {
            ids.extend(v);
        }
        if ids.is_empty()
            && let Some(v) = self.movies_title.get(&(normalize(title), year))
        {
            ids.extend(v);
        }
        self.server_names(ids)
    }

    /// The (server, series key) pairs that are this series.
    fn series_keys(
        &self,
        tmdb: Option<i64>,
        tvdb: Option<i64>,
        imdb: Option<&str>,
        title: &str,
    ) -> Vec<(i64, &str)> {
        let by_id: Vec<(i64, &str)> = self
            .series
            .iter()
            .filter(|s| {
                (tmdb.is_some() && s.tmdb == tmdb)
                    || (tvdb.is_some() && s.tvdb == tvdb)
                    || (imdb.is_some() && s.imdb.as_deref() == imdb)
            })
            .map(|s| (s.server, s.key.as_str()))
            .collect();
        if !by_id.is_empty() {
            return by_id;
        }
        // A server entry with no ids at all can only be matched by its title.
        let t = normalize(title);
        self.series
            .iter()
            .filter(|s| s.tmdb.is_none() && s.tvdb.is_none() && s.imdb.is_none() && s.title == t)
            .map(|s| (s.server, s.key.as_str()))
            .collect()
    }

    pub fn series(
        &self,
        tmdb: Option<i64>,
        tvdb: Option<i64>,
        imdb: Option<&str>,
        title: &str,
    ) -> Vec<String> {
        self.server_names(
            self.series_keys(tmdb, tvdb, imdb, title)
                .into_iter()
                .map(|(s, _)| s),
        )
    }

    /// For each season and episode of a series: the servers that have it.
    pub fn episodes(
        &self,
        tmdb: Option<i64>,
        tvdb: Option<i64>,
        imdb: Option<&str>,
        title: &str,
    ) -> HashMap<(i64, i64), Vec<String>> {
        let mut out: HashMap<(i64, i64), Vec<i64>> = HashMap::new();
        for (server, key) in self.series_keys(tmdb, tvdb, imdb, title) {
            for ep in self
                .episodes
                .get(&(server, key.to_owned()))
                .into_iter()
                .flatten()
            {
                out.entry(*ep).or_default().push(server);
            }
        }
        out.into_iter()
            .map(|(k, v)| (k, self.server_names(v)))
            .collect()
    }

    /// How many episodes of a series the servers hold.
    pub fn episode_count(&self, series: &[(i64, &str)]) -> usize {
        series
            .iter()
            .filter_map(|(s, k)| self.episodes.get(&(*s, (*k).to_owned())))
            .flatten()
            .collect::<HashSet<_>>()
            .len()
    }
}

fn int(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(Value::as_i64).filter(|n| *n > 0)
}

/// Adds `media_servers` (the names of the servers that hold it) to a movie or lookup result.
pub fn annotate_movie(presence: &Presence, v: &mut Value) {
    let found = presence.movie(
        int(v, "tmdb_id"),
        v.get("imdb_id").and_then(Value::as_str),
        v.get("title").and_then(Value::as_str).unwrap_or_default(),
        v.get("year").and_then(Value::as_i64).unwrap_or(0),
    );
    v["media_servers"] = json!(found);
}

/// As [`annotate_movie`] for a series, with the count of episodes held.
pub fn annotate_series(presence: &Presence, v: &mut Value) {
    let (tmdb, tvdb) = (int(v, "tmdb_id"), int(v, "tvdb_id"));
    let imdb = v.get("imdb_id").and_then(Value::as_str).map(str::to_owned);
    let title = v
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let keys = presence.series_keys(tmdb, tvdb, imdb.as_deref(), &title);
    let servers = presence.series(tmdb, tvdb, imdb.as_deref(), &title);
    let count = presence.episode_count(&keys);
    v["media_servers"] = json!(servers);
    v["media_server_episodes"] = json!(count);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presence() -> Presence {
        let mut p = Presence::default();
        p.names.insert(1, "Plex".into());
        p.names.insert(2, "Jellyfin".into());
        p.movies_tmdb.insert(603, vec![1, 2]);
        p.movies_imdb.insert("tt0133093".into(), vec![1]);
        p.movies_title
            .insert((normalize("Spider-Man: Homecoming"), 2017), vec![2]);
        p.series.push(SeriesEntry {
            server: 1,
            key: "10".into(),
            tmdb: Some(1396),
            tvdb: Some(81189),
            imdb: None,
            title: normalize("Breaking Bad"),
        });
        p.series.push(SeriesEntry {
            server: 2,
            key: "abc".into(),
            tmdb: None,
            tvdb: None,
            imdb: None,
            title: normalize("The Wire"),
        });
        p.episodes
            .insert((1, "10".into()), HashSet::from([(1, 1), (1, 2)]));
        p.episodes
            .insert((2, "abc".into()), HashSet::from([(1, 1)]));
        p
    }

    #[test]
    fn a_movie_is_found_by_tmdb_then_imdb_then_title_and_year() {
        let p = presence();
        assert_eq!(p.movie(Some(603), None, "x", 0), ["Jellyfin", "Plex"]);
        assert_eq!(p.movie(None, Some("tt0133093"), "x", 0), ["Plex"]);
        assert_eq!(
            p.movie(None, None, "Spider Man Homecoming", 2017),
            ["Jellyfin"]
        );
        assert!(
            p.movie(None, None, "Spider Man Homecoming", 2018)
                .is_empty()
        );
        assert!(p.movie(Some(1), None, "Nothing", 2000).is_empty());
    }

    #[test]
    fn a_title_only_matches_when_the_server_gave_no_ids() {
        let p = presence();
        assert_eq!(p.series(None, None, None, "the wire"), ["Jellyfin"]);
        // Breaking Bad has ids on the server, so its name alone is not enough.
        assert!(p.series(None, None, None, "Breaking Bad").is_empty());
        assert_eq!(p.series(None, Some(81189), None, "anything"), ["Plex"]);
    }

    #[test]
    fn episodes_name_the_servers_that_have_them() {
        let p = presence();
        let e = p.episodes(Some(1396), None, None, "Breaking Bad");
        assert_eq!(e[&(1, 1)], ["Plex"]);
        assert_eq!(e[&(1, 2)], ["Plex"]);
        assert!(!e.contains_key(&(1, 3)));
        let wire = p.episodes(None, None, None, "The Wire");
        assert_eq!(wire[&(1, 1)], ["Jellyfin"]);
    }

    #[test]
    fn annotating_adds_servers_and_episode_counts() {
        let p = presence();
        let mut m = json!({"tmdb_id": 603, "title": "The Matrix", "year": 1999});
        annotate_movie(&p, &mut m);
        assert_eq!(m["media_servers"], json!(["Jellyfin", "Plex"]));
        let mut s = json!({"tmdb_id": 1396, "tvdb_id": 81189, "title": "Breaking Bad"});
        annotate_series(&p, &mut s);
        assert_eq!(s["media_servers"], json!(["Plex"]));
        assert_eq!(s["media_server_episodes"], json!(2));
    }
}
