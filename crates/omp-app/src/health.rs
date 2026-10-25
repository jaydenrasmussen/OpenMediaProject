//! "Is anything wrong?": the checks behind the health banner. Modelled on
//! Radarr's health checks, limited to what this app can actually get wrong.

use std::path::Path;

use chrono::Utc;
use omp_db::repo::ProviderKind;
use omp_services::TrackedState;
use serde::Serialize;
use tokio::process::Command;

use crate::app::App;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthIssue {
    /// What checked it ("RootFolders", "Indexers", ...).
    pub source: &'static str,
    pub level: Level,
    pub message: String,
}

fn issue(source: &'static str, level: Level, message: impl Into<String>) -> HealthIssue {
    HealthIssue {
        source,
        level,
        message: message.into(),
    }
}

/// Can the program be started at all? (The exit status does not matter: some
/// tools return non-zero for `--help`.)
async fn tool_runs(tool: &Path, arg: &str) -> bool {
    Command::new(tool)
        .arg(arg)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .status()
        .await
        .is_ok()
}

const LOW_DISK_BYTES: u64 = 1024 * 1024 * 1024;

pub async fn check(app: &App) -> Result<Vec<HealthIssue>, omp_db::DbError> {
    let mut issues = Vec::new();
    let now = Utc::now();

    for problem in app.plugins().problems() {
        issues.push(issue(
            "Plugins",
            Level::Warning,
            format!("A plugin could not be loaded: {problem}"),
        ));
    }

    let vpn = crate::vpn::status().await;
    if vpn.enabled && vpn.status.as_deref() != Some("running") {
        issues.push(issue(
            "Vpn",
            Level::Error,
            format!(
                "The VPN is not connected (status: {}). The firewall blocks everything else, so nothing can be reached until it is.",
                vpn.status.as_deref().unwrap_or("unknown")
            ),
        ));
    }

    if app.metadata.is_none() {
        issues.push(issue(
            "Metadata",
            Level::Error,
            "No TMDB access token is configured (TMDB_READ_TOKEN), so movies cannot be added or refreshed.",
        ));
    }

    // Root folders.
    let roots = app.db.root_folders().list().await?;
    if roots.is_empty() {
        issues.push(issue(
            "RootFolders",
            Level::Error,
            "No root folder is configured, so movies have nowhere to go.",
        ));
    }
    for r in &roots {
        let path = Path::new(&r.path);
        if !path.is_dir() {
            issues.push(issue(
                "RootFolders",
                Level::Error,
                format!("Root folder {} is missing.", r.path),
            ));
            continue;
        }
        if let Some(free) = app.disk.free_bytes(&r.path)
            && free < LOW_DISK_BYTES
        {
            issues.push(issue(
                "DiskSpace",
                Level::Warning,
                format!("Only {:.1} GB is free in {}.", free as f64 / 1e9, r.path),
            ));
        }
    }

    // Indexers.
    let indexers = app.db.providers().list(ProviderKind::Indexer).await?;
    let enabled: Vec<_> = indexers.iter().filter(|p| p.enabled).collect();
    if enabled.is_empty() {
        issues.push(issue(
            "Indexers",
            Level::Error,
            "No indexers are enabled, so nothing can be found.",
        ));
    } else {
        if !enabled.iter().any(|p| p.enable_rss) {
            issues.push(issue("Indexers", Level::Warning, "No indexer has RSS sync enabled, so new releases will not be picked up on their own."));
        }
        if !enabled.iter().any(|p| p.enable_automatic_search) {
            issues.push(issue(
                "Indexers",
                Level::Warning,
                "No indexer has automatic search enabled.",
            ));
        }
        if !enabled.iter().any(|p| p.enable_interactive_search) {
            issues.push(issue(
                "Indexers",
                Level::Warning,
                "No indexer has interactive search enabled.",
            ));
        }
    }
    let blocked = app
        .db
        .providers()
        .blocked(ProviderKind::Indexer, now)
        .await?;
    for p in indexers
        .iter()
        .filter(|p| blocked.iter().any(|(id, _)| *id == p.id))
    {
        let till = blocked
            .iter()
            .find(|(id, _)| *id == p.id)
            .map(|(_, t)| *t)
            .unwrap_or(now);
        issues.push(issue(
            "Indexers",
            Level::Warning,
            format!(
                "{} is paused after recent failures until {}.",
                p.name,
                till.format("%H:%M UTC")
            ),
        ));
    }

    // Download clients.
    let clients = app
        .db
        .providers()
        .list(ProviderKind::DownloadClient)
        .await?;
    let running = app.registry();
    let enabled_clients: Vec<_> = clients.iter().filter(|c| c.enabled).collect();
    if enabled_clients.is_empty() {
        issues.push(issue(
            "DownloadClients",
            Level::Error,
            "No download client is enabled, so nothing can be downloaded.",
        ));
    }
    // Newznab indexers are the only reason to need news servers.
    let usenet_indexers = app
        .db
        .providers()
        .list(ProviderKind::Indexer)
        .await?
        .iter()
        .any(|i| i.enabled && i.implementation == "newznab");
    for c in &enabled_clients {
        if running.get(c.id).is_none() {
            // The built-in usenet downloader starts with no news server, which
            // is not a fault until a usenet indexer wants to use it.
            let no_servers = c.implementation == "usenet"
                && c.settings["servers"].as_array().is_none_or(Vec::is_empty);
            if no_servers {
                if usenet_indexers {
                    issues.push(issue(
                        "DownloadClients",
                        Level::Warning,
                        format!(
                            "{} has no news server yet, so usenet results cannot be downloaded. Add your provider in Settings → Download clients.",
                            c.name
                        ),
                    ));
                }
                continue;
            }
            issues.push(issue(
                "DownloadClients",
                Level::Error,
                format!(
                    "{} could not be started. Check its settings with the Test button.",
                    c.name
                ),
            ));
        }
    }
    let blocked = app
        .db
        .providers()
        .blocked(ProviderKind::DownloadClient, now)
        .await?;
    for c in clients
        .iter()
        .filter(|c| blocked.iter().any(|(id, _)| *id == c.id))
    {
        issues.push(issue(
            "DownloadClients",
            Level::Warning,
            format!("{} is unavailable after recent failures.", c.name),
        ));
    }

    // Tools the downloads and imports rely on.
    if !tool_runs(&app.config.ffprobe, "-version").await {
        issues.push(issue(
            "Tools",
            Level::Error,
            "ffprobe cannot be run, so downloaded files cannot be checked and will not be imported.",
        ));
    }
    if !tool_runs(&app.config.par2, "--version").await {
        issues.push(issue(
            "Tools",
            Level::Warning,
            "par2 cannot be run, so damaged usenet downloads cannot be repaired.",
        ));
    }
    if !tool_runs(&app.config.sevenzip, "--help").await {
        issues.push(issue(
            "Tools",
            Level::Warning,
            "7-Zip (7zz) cannot be run, so archives in usenet downloads cannot be unpacked.",
        ));
    }

    // Downloads waiting on a person.
    let stuck = app
        .monitor
        .snapshot()
        .into_iter()
        .filter(|t| t.state == TrackedState::ImportBlocked)
        .count();
    if stuck > 0 {
        issues.push(issue(
            "Downloads",
            Level::Warning,
            format!(
                "{stuck} finished download{} cannot be imported automatically. See Activity.",
                if stuck == 1 { "" } else { "s" }
            ),
        ));
    }

    issues.sort_by_key(|i| std::cmp::Reverse(i.level));
    Ok(issues)
}
