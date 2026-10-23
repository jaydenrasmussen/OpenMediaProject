//! An OpenAPI description of the HTTP API, served at `/api/v1/openapi.json`.
//!
//! The table below is written by hand. Two tests keep it honest: every
//! entry must be a route the server really has, and every route written in
//! the source must be in the table. Request and response bodies are described
//! only as JSON objects; the UI's `types.ts` and the tests are the detailed
//! reference for their fields.

use axum::Json;
use serde_json::{Value, json};

/// (method, path, tag, summary)
type Entry = (&'static str, &'static str, &'static str, &'static str);

const PROVIDERS: [(&str, &str); 4] = [
    ("indexer", "Indexers"),
    ("downloadclient", "Download clients"),
    ("notification", "Notifications"),
    ("importlist", "Import lists"),
];

pub(crate) const ROUTES: &[Entry] = &[
    (
        "get",
        "/health",
        "System",
        "Liveness check (no login needed)",
    ),
    (
        "get",
        "/auth/status",
        "Authentication",
        "Whether setup or login is needed",
    ),
    (
        "post",
        "/auth/setup",
        "Authentication",
        "Create the first account",
    ),
    (
        "post",
        "/auth/login",
        "Authentication",
        "Sign in and get a session cookie",
    ),
    ("post", "/auth/logout", "Authentication", "End the session"),
    (
        "post",
        "/auth/password",
        "Authentication",
        "Change the password",
    ),
    ("get", "/auth/apikey", "Authentication", "Read the API key"),
    (
        "post",
        "/auth/apikey/regenerate",
        "Authentication",
        "Replace the API key",
    ),
    ("get", "/openapi.json", "System", "This description"),
    (
        "get",
        "/ws",
        "System",
        "WebSocket that announces changes: {\"kind\":\"queue|history|tasks|changed\"}",
    ),
    (
        "get",
        "/system/status",
        "System",
        "Version and runtime facts",
    ),
    (
        "get",
        "/system/health",
        "System",
        "Problems that need attention",
    ),
    (
        "get",
        "/system/task",
        "System",
        "Scheduled jobs and their last run",
    ),
    ("post", "/system/restart", "System", "Restart the server"),
    ("get", "/system/backup", "System", "List backups"),
    ("post", "/system/backup", "System", "Create a backup"),
    (
        "get",
        "/system/backup/{name}",
        "System",
        "Download a backup",
    ),
    (
        "delete",
        "/system/backup/{name}",
        "System",
        "Delete a backup",
    ),
    (
        "post",
        "/system/backup/{name}/restore",
        "System",
        "Restore a backup and restart",
    ),
    ("get", "/log", "System", "Event log"),
    ("delete", "/log", "System", "Clear the event log"),
    (
        "post",
        "/command",
        "System",
        "Run a job: RssSync, MoviesSearch, SeriesSearch, ...",
    ),
    (
        "get",
        "/remotepathmapping",
        "Download clients",
        "Remote path mappings",
    ),
    (
        "post",
        "/remotepathmapping",
        "Download clients",
        "Add a remote path mapping",
    ),
    (
        "delete",
        "/remotepathmapping/{id}",
        "Download clients",
        "Delete a remote path mapping",
    ),
    ("get", "/plugin", "Plugins", "Installed plugins"),
    (
        "post",
        "/plugin/install",
        "Plugins",
        "Preview (no sha256) or install (with the previewed sha256) a plugin zip from a URL",
    ),
    (
        "delete",
        "/plugin/{id}",
        "Plugins",
        "Remove an installed (not bundled) plugin",
    ),
    (
        "get",
        "/plugin/{id}/schema",
        "Plugins",
        "A plugin's settings schema",
    ),
    ("get", "/movie", "Movies", "The movie library"),
    ("post", "/movie", "Movies", "Add a movie"),
    (
        "get",
        "/movie/lookup",
        "Movies",
        "Search TMDB (title, tmdb:ID or imdb:ID)",
    ),
    (
        "post",
        "/movie/import",
        "Movies",
        "Adopt existing movie folders",
    ),
    ("get", "/movie/{id}", "Movies", "One movie"),
    ("put", "/movie/{id}", "Movies", "Edit a movie"),
    ("delete", "/movie/{id}", "Movies", "Remove a movie"),
    (
        "delete",
        "/movie/{id}/file",
        "Movies",
        "Delete a movie's file",
    ),
    ("get", "/series", "Series", "The series library"),
    ("post", "/series", "Series", "Add a series"),
    (
        "get",
        "/series/lookup",
        "Series",
        "Search for a series (title, tmdb:ID, tvdb:ID, imdb:ID)",
    ),
    (
        "post",
        "/series/import",
        "Series",
        "Adopt existing series folders",
    ),
    (
        "put",
        "/series/editor",
        "Series",
        "Change many series at once",
    ),
    (
        "get",
        "/series/{id}",
        "Series",
        "One series with its seasons",
    ),
    ("put", "/series/{id}", "Series", "Edit a series"),
    ("delete", "/series/{id}", "Series", "Remove a series"),
    (
        "put",
        "/series/{id}/season/{season}",
        "Series",
        "Monitor or unmonitor a season",
    ),
    (
        "post",
        "/series/{id}/monitor",
        "Series",
        "Apply a monitoring mode",
    ),
    (
        "get",
        "/episode",
        "Series",
        "Episodes of a series (series_id=)",
    ),
    (
        "put",
        "/episode/monitor",
        "Series",
        "Monitor or unmonitor episodes",
    ),
    ("get", "/episode/{id}", "Series", "One episode"),
    (
        "delete",
        "/episodefile/{id}",
        "Series",
        "Delete an episode file",
    ),
    ("get", "/rootfolder", "Root folders", "Root folders"),
    ("post", "/rootfolder", "Root folders", "Add a root folder"),
    (
        "delete",
        "/rootfolder/{id}",
        "Root folders",
        "Remove a root folder",
    ),
    (
        "get",
        "/rootfolder/{id}/unmapped",
        "Root folders",
        "Movie folders not in the library",
    ),
    (
        "get",
        "/rootfolder/{id}/unmapped-series",
        "Root folders",
        "Series folders not in the library",
    ),
    (
        "get",
        "/release",
        "Releases",
        "Interactive movie search (movie_id=)",
    ),
    (
        "post",
        "/release",
        "Releases",
        "Grab a release from a movie search",
    ),
    (
        "get",
        "/release/episode",
        "Releases",
        "Interactive episode or season search",
    ),
    (
        "post",
        "/release/episode",
        "Releases",
        "Grab a release from an episode search",
    ),
    (
        "post",
        "/release/push",
        "Releases",
        "Offer a release for evaluation and grabbing",
    ),
    (
        "get",
        "/manualimport",
        "Activity",
        "Files in a download and what they look like",
    ),
    (
        "post",
        "/manualimport",
        "Activity",
        "Import chosen files by hand",
    ),
    ("get", "/queue", "Activity", "Downloads in progress"),
    (
        "delete",
        "/queue/{download_id}",
        "Activity",
        "Remove a download (optionally blocklist)",
    ),
    ("get", "/history", "Activity", "Movie history"),
    ("get", "/history/episode", "Activity", "Episode history"),
    (
        "get",
        "/blocklist",
        "Activity",
        "Blocklisted movie releases",
    ),
    (
        "delete",
        "/blocklist",
        "Activity",
        "Clear the movie blocklist",
    ),
    (
        "delete",
        "/blocklist/{id}",
        "Activity",
        "Unblock a movie release",
    ),
    (
        "get",
        "/blocklist/series",
        "Activity",
        "Blocklisted episode releases",
    ),
    (
        "delete",
        "/blocklist/series",
        "Activity",
        "Clear the episode blocklist",
    ),
    (
        "delete",
        "/blocklist/series/{id}",
        "Activity",
        "Unblock an episode release",
    ),
    (
        "get",
        "/wanted/missing",
        "Wanted",
        "Monitored movies without a file",
    ),
    (
        "get",
        "/wanted/cutoff",
        "Wanted",
        "Movies below their cutoff",
    ),
    (
        "get",
        "/wanted/episodes/missing",
        "Wanted",
        "Aired, monitored episodes without a file",
    ),
    (
        "get",
        "/wanted/episodes/cutoff",
        "Wanted",
        "Episodes below their cutoff",
    ),
    (
        "get",
        "/calendar/episodes",
        "Calendar",
        "Episodes airing between start= and end=",
    ),
    (
        "get",
        "/calendar/feed.ics",
        "Calendar",
        "iCalendar feed of releases and episodes",
    ),
    (
        "get",
        "/rename",
        "Rename",
        "Movie files that would be renamed",
    ),
    (
        "get",
        "/rename/episodes",
        "Rename",
        "Episode files that would be renamed",
    ),
    ("get", "/qualityprofile", "Profiles", "Quality profiles"),
    (
        "post",
        "/qualityprofile",
        "Profiles",
        "Add a quality profile",
    ),
    (
        "get",
        "/qualityprofile/{id}",
        "Profiles",
        "One quality profile",
    ),
    (
        "put",
        "/qualityprofile/{id}",
        "Profiles",
        "Edit a quality profile",
    ),
    (
        "delete",
        "/qualityprofile/{id}",
        "Profiles",
        "Delete a quality profile",
    ),
    ("get", "/customformat", "Profiles", "Custom formats"),
    ("post", "/customformat", "Profiles", "Add a custom format"),
    (
        "put",
        "/customformat/{id}",
        "Profiles",
        "Edit a custom format",
    ),
    (
        "delete",
        "/customformat/{id}",
        "Profiles",
        "Delete a custom format",
    ),
    ("get", "/delayprofile", "Profiles", "Delay profiles"),
    ("post", "/delayprofile", "Profiles", "Add a delay profile"),
    (
        "put",
        "/delayprofile/{id}",
        "Profiles",
        "Edit a delay profile",
    ),
    (
        "delete",
        "/delayprofile/{id}",
        "Profiles",
        "Delete a delay profile",
    ),
    ("get", "/releaseprofile", "Profiles", "Release profiles"),
    (
        "post",
        "/releaseprofile",
        "Profiles",
        "Add a release profile",
    ),
    (
        "put",
        "/releaseprofile/{id}",
        "Profiles",
        "Edit a release profile",
    ),
    (
        "delete",
        "/releaseprofile/{id}",
        "Profiles",
        "Delete a release profile",
    ),
    (
        "get",
        "/qualitydefinition",
        "Profiles",
        "Quality size definitions",
    ),
    (
        "put",
        "/qualitydefinition/{quality_id}",
        "Profiles",
        "Edit a size definition",
    ),
    ("get", "/tag", "Tags", "Tags"),
    ("post", "/tag", "Tags", "Add a tag"),
    ("delete", "/tag/{id}", "Tags", "Delete a tag"),
    (
        "get",
        "/config/import",
        "Settings",
        "Importing and naming settings",
    ),
    (
        "put",
        "/config/import",
        "Settings",
        "Save importing and naming settings",
    ),
    (
        "get",
        "/config/decision",
        "Settings",
        "Release selection settings",
    ),
    (
        "put",
        "/config/decision",
        "Settings",
        "Save release selection settings",
    ),
    (
        "get",
        "/exclusion",
        "Import lists",
        "Import list exclusions",
    ),
    (
        "delete",
        "/exclusion/{tmdb_id}",
        "Import lists",
        "Lift an exclusion",
    ),
];

/// The routes shared by the four provider kinds.
fn provider_entries() -> Vec<(String, String, String, String)> {
    let mut out = Vec::new();
    for (path, tag) in PROVIDERS {
        let noun = tag.to_lowercase();
        for (method, suffix, what) in [
            ("get", "", format!("List {noun}")),
            ("post", "", format!("Add to {noun}")),
            (
                "post",
                "/test",
                format!("Test settings for {noun} without saving"),
            ),
            ("put", "/{id}", format!("Edit one of the {noun}")),
            ("delete", "/{id}", format!("Delete one of the {noun}")),
        ] {
            out.push((
                method.to_owned(),
                format!("/{path}{suffix}"),
                tag.to_owned(),
                what,
            ));
        }
    }
    out
}

/// Every (method, path) the table describes, with the provider routes.
pub(crate) fn all_entries() -> Vec<(String, String, String, String)> {
    let mut v: Vec<_> = ROUTES
        .iter()
        .map(|(m, p, t, s)| {
            (
                (*m).to_owned(),
                (*p).to_owned(),
                (*t).to_owned(),
                (*s).to_owned(),
            )
        })
        .collect();
    v.extend(provider_entries());
    v
}

fn parameters(path: &str) -> Vec<Value> {
    path.split('/')
        .filter_map(|seg| seg.strip_prefix('{')?.strip_suffix('}'))
        .map(|name| {
            let numeric = name == "id" || name == "season" || name.ends_with("_id");
            json!({
                "name": name,
                "in": "path",
                "required": true,
                "schema": { "type": if numeric && name != "download_id" { "integer" } else { "string" } },
            })
        })
        .collect()
}

pub fn document() -> Value {
    let mut paths = serde_json::Map::new();
    for (method, path, tag, summary) in all_entries() {
        let public = matches!(
            path.as_str(),
            "/health" | "/auth/status" | "/auth/setup" | "/auth/login"
        );
        let mut op = json!({
            "tags": [tag],
            "summary": summary,
            "parameters": parameters(&path),
            "responses": {
                "200": { "description": "Success", "content": { "application/json": { "schema": { "type": "object" } } } },
                "401": { "description": "Not signed in and no valid API key" },
            },
        });
        if matches!(method.as_str(), "post" | "put") {
            op["requestBody"] = json!({
                "required": false,
                "content": { "application/json": { "schema": { "type": "object" } } },
            });
        }
        if public {
            op["security"] = json!([]);
        }
        paths
            .entry(path)
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("an object")
            .insert(method, op);
    }
    json!({
        "openapi": "3.0.3",
        "info": {
            "title": "OpenMediaProject API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Send the API key in the X-Api-Key header (or ?apikey=), or use the session cookie from /auth/login. Bodies are JSON.",
        },
        "servers": [{ "url": "/api/v1" }],
        "security": [{ "apiKey": [] }, { "session": [] }],
        "components": { "securitySchemes": {
            "apiKey": { "type": "apiKey", "in": "header", "name": "X-Api-Key" },
            "session": { "type": "apiKey", "in": "cookie", "name": "omp_session" },
        } },
        "paths": paths,
    })
}

pub async fn serve() -> Json<Value> {
    Json(document())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every path given to a router `route` call in the source.
    fn paths_in_source() -> Vec<String> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let mut rest = text.as_str();
            while let Some(i) = rest.find(".route(") {
                rest = &rest[i + 7..];
                let trimmed = rest.trim_start();
                if let Some(body) = trimmed.strip_prefix('"')
                    && let Some(end) = body.find('"')
                    && body.starts_with('/')
                {
                    found.push(body[..end].to_owned());
                }
            }
        }
        found
    }

    #[test]
    fn every_route_in_the_source_is_described() {
        let described: Vec<String> = all_entries().into_iter().map(|e| e.1).collect();
        let missing: Vec<_> = paths_in_source()
            .into_iter()
            .filter(|p| !described.contains(p))
            .collect();
        assert!(missing.is_empty(), "not in the OpenAPI table: {missing:?}");
    }

    #[test]
    fn the_document_has_every_operation_once() {
        let doc = document();
        let entries = all_entries();
        let mut operations = 0;
        for (_, item) in doc["paths"].as_object().unwrap() {
            operations += item.as_object().unwrap().len();
        }
        assert_eq!(
            operations,
            entries.len(),
            "duplicate (method, path) pairs in the table"
        );
        assert_eq!(
            doc["paths"]["/series/{id}/season/{season}"]["put"]["parameters"][1]["name"],
            "season"
        );
    }
}
