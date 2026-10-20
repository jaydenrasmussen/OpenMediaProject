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
    #[serde(default)]
    pub settings: Value,
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
