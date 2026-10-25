//! Database backups: consistent copies of the main database, kept in
//! `<data dir>/backups`. Restoring is a file swap done by hand (see the docs),
//! because the running app holds the database open.

use std::path::PathBuf;

use chrono::Utc;
use serde::Serialize;

use crate::app::App;

/// How many backups to keep; the oldest go first.
pub const KEEP: usize = 7;

#[derive(Debug, Clone, Serialize)]
pub struct BackupInfo {
    pub name: String,
    pub size: u64,
    pub created: chrono::DateTime<Utc>,
}

fn dir(app: &App) -> PathBuf {
    app.config.data_dir.join("backups")
}

/// Only names we make ourselves are served, deleted or listed, so a request
/// cannot reach any other file.
pub fn is_backup_name(name: &str) -> bool {
    name.strip_prefix("omp_backup_")
        .and_then(|rest| rest.strip_suffix(".db"))
        .is_some_and(|stamp| {
            !stamp.is_empty() && stamp.chars().all(|c| c.is_ascii_digit() || c == '_')
        })
}

pub fn path_of(app: &App, name: &str) -> Option<PathBuf> {
    is_backup_name(name).then(|| dir(app).join(name))
}

pub async fn list(app: &App) -> std::io::Result<Vec<BackupInfo>> {
    let mut out = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(dir(app)).await else {
        return Ok(out);
    };
    while let Some(e) = entries.next_entry().await? {
        let name = e.file_name().to_string_lossy().into_owned();
        if !is_backup_name(&name) {
            continue;
        }
        let meta = e.metadata().await?;
        let created = meta
            .modified()
            .map(chrono::DateTime::<Utc>::from)
            .unwrap_or_else(|_| Utc::now());
        out.push(BackupInfo {
            name,
            size: meta.len(),
            created,
        });
    }
    out.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(out)
}

/// Makes a backup now and removes old ones beyond [`KEEP`].
pub async fn create(app: &App) -> Result<BackupInfo, String> {
    let dir = dir(app);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| e.to_string())?;
    let mut name = format!("omp_backup_{}.db", Utc::now().format("%Y%m%d_%H%M%S"));
    // Two backups in one second get distinct names.
    let mut n = 1;
    while dir.join(&name).exists() {
        name = format!("omp_backup_{}_{n}.db", Utc::now().format("%Y%m%d_%H%M%S"));
        n += 1;
    }
    let path = dir.join(&name);
    app.db.backup_to(&path).await.map_err(|e| e.to_string())?;
    prune(app).await.map_err(|e| e.to_string())?;
    let meta = tokio::fs::metadata(&path)
        .await
        .map_err(|e| e.to_string())?;
    Ok(BackupInfo {
        name,
        size: meta.len(),
        created: Utc::now(),
    })
}

async fn prune(app: &App) -> std::io::Result<()> {
    for old in list(app).await?.into_iter().skip(KEEP) {
        tokio::fs::remove_file(dir(app).join(old.name)).await?;
    }
    Ok(())
}
