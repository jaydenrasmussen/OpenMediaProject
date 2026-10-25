//! Getting rid of files: into the recycle bin when one is set up, otherwise
//! for good. Old files replaced by an upgrade and files the user deletes go
//! through here.

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use crate::import::ImportConfig;

/// Moves a file or folder, copying across file systems when a plain rename
/// cannot.
async fn move_path(from: &Path, to: &Path) -> std::io::Result<()> {
    if tokio::fs::rename(from, to).await.is_ok() {
        return Ok(());
    }
    if from.is_dir() {
        copy_dir(from, to).await?;
        tokio::fs::remove_dir_all(from).await
    } else {
        tokio::fs::copy(from, to).await?;
        tokio::fs::remove_file(from).await
    }
}

async fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut pending = vec![(from.to_path_buf(), to.to_path_buf())];
    while let Some((src, dst)) = pending.pop() {
        tokio::fs::create_dir_all(&dst).await?;
        let mut entries = tokio::fs::read_dir(&src).await?;
        while let Some(entry) = entries.next_entry().await? {
            let target = dst.join(entry.file_name());
            if entry.file_type().await?.is_dir() {
                pending.push((entry.path(), target));
            } else {
                tokio::fs::copy(entry.path(), target).await?;
            }
        }
    }
    Ok(())
}

/// A name in the bin that is not taken yet.
fn free_name(bin: &Path, name: &str) -> PathBuf {
    let first = bin.join(name);
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| bin.join(format!("{name}.{n}")))
        .find(|p| !p.exists())
        .expect("an unused name exists")
}

/// Deletes `path` (a file or a folder), or moves it to the recycle bin.
/// `name` is what it is called in the bin.
pub async fn dispose(config: &ImportConfig, path: &Path, name: &str) -> std::io::Result<()> {
    let bin = config.recycle_bin.trim();
    if bin.is_empty() {
        return if path.is_dir() {
            tokio::fs::remove_dir_all(path).await
        } else {
            tokio::fs::remove_file(path).await
        };
    }
    let bin = Path::new(bin);
    tokio::fs::create_dir_all(bin).await?;
    let target = free_name(bin, name);
    move_path(path, &target).await?;
    // Cleanup goes by modification time, so the move should count as "now".
    let now = filetime_now();
    let _ = tokio::task::spawn_blocking(move || touch(&target, now)).await;
    Ok(())
}

fn filetime_now() -> SystemTime {
    SystemTime::now()
}

fn touch(path: &Path, when: SystemTime) {
    // Read-only is enough for the owner to set the time, and works for folders too.
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.set_modified(when);
    }
}

/// Removes what has sat in the bin longer than the configured days. Returns
/// how many items were removed.
pub async fn clean_bin(config: &ImportConfig) -> std::io::Result<usize> {
    let bin = config.recycle_bin.trim();
    if bin.is_empty() || config.recycle_bin_cleanup_days == 0 {
        return Ok(0);
    }
    let limit = Duration::from_secs(u64::from(config.recycle_bin_cleanup_days) * 86_400);
    let mut removed = 0;
    let mut entries = match tokio::fs::read_dir(bin).await {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    while let Some(entry) = entries.next_entry().await? {
        let age = entry
            .metadata()
            .await?
            .modified()
            .ok()
            .and_then(|m| SystemTime::now().duration_since(m).ok());
        if age.is_some_and(|a| a > limit) {
            let path = entry.path();
            let done = if path.is_dir() {
                tokio::fs::remove_dir_all(&path).await
            } else {
                tokio::fs::remove_file(&path).await
            };
            if done.is_ok() {
                removed += 1;
            }
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(bin: &Path) -> ImportConfig {
        ImportConfig {
            recycle_bin: bin.to_string_lossy().into_owned(),
            recycle_bin_cleanup_days: 7,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn without_a_bin_files_are_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.mkv");
        std::fs::write(&file, b"x").unwrap();
        dispose(&ImportConfig::default(), &file, "a.mkv")
            .await
            .unwrap();
        assert!(!file.exists());
    }

    #[tokio::test]
    async fn with_a_bin_files_and_folders_move_there_without_clobbering() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let cfg = config(&bin);
        for round in 0..2u8 {
            let file = dir.path().join("a.mkv");
            std::fs::write(&file, [round]).unwrap();
            dispose(&cfg, &file, "a.mkv").await.unwrap();
            assert!(!file.exists());
        }
        assert_eq!(std::fs::read(bin.join("a.mkv")).unwrap(), [0]);
        assert_eq!(std::fs::read(bin.join("a.mkv.2")).unwrap(), [1]);

        let folder = dir.path().join("Movie (2000)");
        std::fs::create_dir_all(folder.join("sub")).unwrap();
        std::fs::write(folder.join("sub/b.mkv"), b"y").unwrap();
        dispose(&cfg, &folder, "Movie (2000)").await.unwrap();
        assert!(!folder.exists());
        assert!(bin.join("Movie (2000)/sub/b.mkv").is_file());
    }

    #[tokio::test]
    async fn cleanup_only_removes_old_items() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let old = bin.join("old.mkv");
        let fresh = bin.join("fresh.mkv");
        std::fs::write(&old, b"x").unwrap();
        std::fs::write(&fresh, b"x").unwrap();
        let long_ago = SystemTime::now() - Duration::from_secs(30 * 86_400);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
        assert_eq!(clean_bin(&config(&bin)).await.unwrap(), 1);
        assert!(!old.exists() && fresh.exists());
        // A bin that was never created is fine, and so is cleanup being off.
        assert_eq!(
            clean_bin(&config(&dir.path().join("none"))).await.unwrap(),
            0
        );
        let mut off = config(&bin);
        off.recycle_bin_cleanup_days = 0;
        assert_eq!(clean_bin(&off).await.unwrap(), 0);
    }
}
