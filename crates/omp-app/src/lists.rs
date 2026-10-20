//! Import lists: plugins that name movies, and what to do with new ones.

use std::{sync::Arc, time::Duration};

use omp_db::repo::{Provider, ProviderKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::{AddMovie, ApiError},
    app::App,
};

fn yes() -> bool {
    true
}

/// How an import list provider's `settings` is laid out: what to do with the
/// movies it names, and the plugin's own settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSettings {
    pub quality_profile_id: i32,
    pub root_folder_path: String,
    #[serde(default = "yes")]
    pub monitored: bool,
    #[serde(default)]
    pub minimum_availability: Option<String>,
    #[serde(default)]
    pub search_on_add: bool,
    #[serde(default)]
    pub tags: Vec<i32>,
    /// Where series the list names go. A list that names series needs these;
    /// without them its series are skipped.
    #[serde(default)]
    pub series_quality_profile_id: Option<i32>,
    #[serde(default)]
    pub series_root_folder_path: Option<String>,
    #[serde(default = "default_monitor")]
    pub series_monitor: String,
    #[serde(default = "yes")]
    pub series_season_folder: bool,
    #[serde(default)]
    pub settings: Value,
}

fn default_monitor() -> String {
    "all".into()
}

impl ListSettings {
    pub fn from_provider(p: &Provider) -> Result<Self, String> {
        serde_json::from_value(p.settings.clone()).map_err(|e| format!("unreadable settings: {e}"))
    }
}

/// Asks a list's plugin for its movies.
pub async fn fetch(app: &App, p: &Provider) -> Result<Vec<omp_plugin_host::ListItem>, String> {
    let plugin_id = p.plugin_id.as_deref().ok_or("no plugin chosen")?;
    let plugin = app
        .plugins
        .list(plugin_id)
        .ok_or_else(|| format!("plugin '{plugin_id}' isn't installed"))?;
    let settings = ListSettings::from_provider(p)?;
    plugin
        .fetch(&settings.settings.to_string())
        .await
        .map_err(|e| format!("{e:#}"))
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ListSyncResult {
    pub added: usize,
    /// Already in the library, or excluded.
    pub skipped: usize,
    /// One message per list or movie that went wrong.
    pub problems: Vec<String>,
}

/// More than this from one list in one sync is not a list, it is a mistake.
const MAX_PER_LIST: usize = 1000;

/// Adds the movies the enabled lists name that the library does not have.
pub async fn sync_once(app: &Arc<App>) -> Result<ListSyncResult, omp_db::DbError> {
    let mut result = ListSyncResult::default();
    for p in app.db.providers().list(ProviderKind::ImportList).await? {
        if !p.enabled {
            continue;
        }
        let settings = match ListSettings::from_provider(&p) {
            Ok(s) => s,
            Err(e) => {
                result.problems.push(format!("{}: {e}", p.name));
                continue;
            }
        };
        let items = match fetch(app, &p).await {
            Ok(items) => items,
            Err(e) => {
                tracing::warn!(list = %p.name, "import list failed: {e}");
                result.problems.push(format!("{}: {e}", p.name));
                continue;
            }
        };
        for item in items.into_iter().take(MAX_PER_LIST) {
            if item.kind == omp_plugin_host::ListItemKind::Series {
                match add_series_item(app, &settings, &item).await {
                    Ok(true) => {
                        tracing::info!(list = %p.name, title = %item.title, "added from import list");
                        result.added += 1;
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                    Ok(false) => result.skipped += 1,
                    Err(message) => result
                        .problems
                        .push(format!("{} ({}): {message}", item.title, p.name)),
                }
                continue;
            }
            if item.tmdb_id <= 0 {
                continue;
            }
            if app.db.import_exclusions().contains(item.tmdb_id).await?
                || app
                    .db
                    .movies()
                    .find_by_tmdb_id(item.tmdb_id)
                    .await?
                    .is_some()
            {
                result.skipped += 1;
                continue;
            }
            let added = crate::api::add_to_library(
                app,
                AddMovie {
                    tmdb_id: item.tmdb_id,
                    quality_profile_id: settings.quality_profile_id,
                    root_folder_path: settings.root_folder_path.clone(),
                    monitored: settings.monitored,
                    minimum_availability: settings.minimum_availability.clone(),
                    tags: settings.tags.clone(),
                    search_now: settings.search_on_add,
                },
            )
            .await;
            match added {
                Ok(_) => {
                    tracing::info!(list = %p.name, title = %item.title, "added from import list");
                    result.added += 1;
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Err(ApiError(_, message)) => {
                    result
                        .problems
                        .push(format!("{} ({}): {message}", item.title, p.name));
                }
            }
        }
    }
    Ok(result)
}

/// Adds a series a list named, unless the library has it. `Ok(false)` means
/// it was already there.
async fn add_series_item(
    app: &Arc<App>,
    settings: &ListSettings,
    item: &omp_plugin_host::ListItem,
) -> Result<bool, String> {
    let (Some(profile), Some(root)) = (
        settings.series_quality_profile_id,
        settings.series_root_folder_path.clone(),
    ) else {
        return Err(
            "this list names series; choose a series quality profile and root folder for it".into(),
        );
    };
    let provider = app
        .series_metadata
        .clone()
        .ok_or("no metadata provider is configured")?;
    // A list may know only the TVDB id.
    let tmdb_id = if item.tmdb_id > 0 {
        item.tmdb_id
    } else {
        let tvdb = item.tvdb_id.ok_or("the list gave no id for this series")?;
        if app
            .db
            .series()
            .find_by_tvdb_id(tvdb)
            .await
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return Ok(false);
        }
        provider
            .series_by_tvdb_id(tvdb)
            .await
            .map_err(|e| e.to_string())?
            .tmdb_id
    };
    if app
        .db
        .series()
        .find_by_tmdb_id(tmdb_id)
        .await
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Ok(false);
    }
    crate::tv_api::add_to_library(
        app,
        crate::tv_api::AddSeriesBody {
            tmdb_id,
            quality_profile_id: profile,
            root_folder_path: root,
            series_type: None,
            monitor: settings.series_monitor.clone(),
            monitored: settings.monitored,
            season_folder: settings.series_season_folder,
            monitor_new_items: "all".into(),
            use_scene_numbering: false,
            tags: settings.tags.clone(),
            search_now: settings.search_on_add,
        },
    )
    .await
    .map(|_| true)
    .map_err(|ApiError(_, message)| message)
}
