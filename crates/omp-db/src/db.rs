use std::{path::Path, str::FromStr, time::Duration};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

use crate::{
    Result,
    repo::{
        Blocklist, Config, CustomFormats, DelayProfiles, EpisodeFiles, EpisodeHistory, Episodes,
        History, ImportExclusions, Logs, MovieFiles, Movies, PendingReleases, PendingTvReleases,
        Providers, QualityDefinitionsRepo, QualityProfiles, ReleaseProfiles, RootFolders, Series,
        Sessions, Tags, TvBlocklist, Users,
    },
};

#[derive(Clone)]
pub struct Db {
    main: SqlitePool,
    logs: SqlitePool,
}

fn options(url: &str) -> Result<SqliteConnectOptions> {
    Ok(SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5)))
}

/// File name a restore waits under until the next start.
const STAGED_RESTORE: &str = "omp.db.restore";

impl Db {
    /// Checks that `backup` is a database this version can run: it opens,
    /// passes SQLite's integrity check and was made by this version or an
    /// older one.
    pub async fn verify_backup(backup: &Path) -> Result<()> {
        let options = SqliteConnectOptions::new()
            .filename(backup)
            .read_only(true)
            .create_if_missing(false);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(|e| crate::DbError::Corrupt(format!("cannot open the backup: {e}")))?;
        let check: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await
            .map_err(|e| crate::DbError::Corrupt(format!("cannot read the backup: {e}")))?;
        if check != "ok" {
            return Err(crate::DbError::Corrupt(format!(
                "the backup is damaged: {check}"
            )));
        }
        let newest: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .map_err(|_| {
                crate::DbError::Corrupt("this is not an OpenMediaProject backup".into())
            })?;
        pool.close().await;
        let known = sqlx::migrate!("./migrations/main")
            .migrations
            .iter()
            .map(|m| m.version)
            .max();
        if newest > known {
            return Err(crate::DbError::Corrupt(
                "the backup comes from a newer version of OpenMediaProject".into(),
            ));
        }
        Ok(())
    }

    /// Lines up `backup` to replace the database the next time the app
    /// starts. The running database is untouched until then.
    pub async fn stage_restore(dir: &Path, backup: &Path) -> Result<()> {
        Self::verify_backup(backup).await?;
        std::fs::copy(backup, dir.join(STAGED_RESTORE))
            .map_err(|e| crate::DbError::Corrupt(format!("cannot stage the restore: {e}")))?;
        Ok(())
    }

    /// Swaps in a staged restore, keeping the database it replaces as
    /// `omp.db.before-restore`. Returns whether there was one.
    fn apply_staged_restore(dir: &Path) -> std::io::Result<bool> {
        let staged = dir.join(STAGED_RESTORE);
        if !staged.is_file() {
            return Ok(false);
        }
        let current = dir.join("omp.db");
        if current.exists() {
            std::fs::rename(&current, dir.join("omp.db.before-restore"))?;
        }
        // Leftover journal files belong to the old database.
        for suffix in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(dir.join(format!("omp.db{suffix}")));
        }
        std::fs::rename(&staged, &current)?;
        Ok(true)
    }

    /// Opens (creating if needed) `omp.db` and `logs.db` in `dir` and
    /// applies pending migrations. A restore staged with
    /// [`Self::stage_restore`] is applied first.
    pub async fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| crate::DbError::Corrupt(e.to_string()))?;
        if Self::apply_staged_restore(dir).map_err(|e| crate::DbError::Corrupt(e.to_string()))? {
            tracing::info!("restored the database from a backup");
        }
        let main = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(options(&format!(
                "sqlite://{}",
                dir.join("omp.db").display()
            ))?)
            .await?;
        let logs = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options(&format!(
                "sqlite://{}",
                dir.join("logs.db").display()
            ))?)
            .await?;
        let db = Self { main, logs };
        db.migrate().await?;
        Ok(db)
    }

    /// A private in-memory database, for tests. One connection, because each
    /// in-memory connection would otherwise be its own database.
    pub async fn in_memory() -> Result<Self> {
        let memory = |_| -> Result<SqliteConnectOptions> {
            Ok(SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true))
        };
        let main = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(memory(())?)
            .await?;
        let logs = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(memory(())?)
            .await?;
        let db = Self { main, logs };
        db.migrate().await?;
        Ok(db)
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations/main").run(&self.main).await?;
        sqlx::migrate!("./migrations/logs").run(&self.logs).await?;
        Ok(())
    }

    pub fn main_pool(&self) -> &SqlitePool {
        &self.main
    }

    pub fn log_pool(&self) -> &SqlitePool {
        &self.logs
    }

    pub async fn close(&self) {
        self.main.close().await;
        self.logs.close().await;
    }

    /// Writes a consistent copy of the main database to `path` (which must
    /// not exist) while the app keeps running. Does nothing useful for the
    /// in-memory test database, which SQLite will not vacuum into a file.
    pub async fn backup_to(&self, path: &Path) -> Result<()> {
        sqlx::query("VACUUM INTO ?")
            .bind(path.to_string_lossy().into_owned())
            .execute(&self.main)
            .await?;
        Ok(())
    }

    pub fn config(&self) -> Config<'_> {
        Config(&self.main)
    }

    pub fn users(&self) -> Users<'_> {
        Users(&self.main)
    }

    pub fn sessions(&self) -> Sessions<'_> {
        Sessions(&self.main)
    }

    pub fn episode_history(&self) -> EpisodeHistory<'_> {
        EpisodeHistory(&self.main)
    }

    pub fn tv_blocklist(&self) -> TvBlocklist<'_> {
        TvBlocklist(&self.main)
    }

    pub fn pending_tv_releases(&self) -> PendingTvReleases<'_> {
        PendingTvReleases(&self.main)
    }

    pub fn series_exclusions(&self) -> crate::repo::SeriesExclusions<'_> {
        crate::repo::SeriesExclusions(&self.main)
    }

    pub fn remote_path_mappings(&self) -> crate::repo::RemotePathMappings<'_> {
        crate::repo::RemotePathMappings(&self.main)
    }

    pub fn scene_titles(&self) -> crate::repo::SceneTitles<'_> {
        crate::repo::SceneTitles(&self.main)
    }

    pub fn series(&self) -> Series<'_> {
        Series(&self.main)
    }

    pub fn episodes(&self) -> Episodes<'_> {
        Episodes(&self.main)
    }

    pub fn episode_files(&self) -> EpisodeFiles<'_> {
        EpisodeFiles(&self.main)
    }

    pub fn logs(&self) -> Logs<'_> {
        Logs(&self.logs)
    }

    pub fn import_exclusions(&self) -> ImportExclusions<'_> {
        ImportExclusions(&self.main)
    }

    pub fn tags(&self) -> Tags<'_> {
        Tags(&self.main)
    }

    pub fn root_folders(&self) -> RootFolders<'_> {
        RootFolders(&self.main)
    }

    pub fn quality_profiles(&self) -> QualityProfiles<'_> {
        QualityProfiles(&self.main)
    }

    pub fn quality_definitions(&self) -> QualityDefinitionsRepo<'_> {
        QualityDefinitionsRepo(&self.main)
    }

    pub fn custom_formats(&self) -> CustomFormats<'_> {
        CustomFormats(&self.main)
    }

    pub fn delay_profiles(&self) -> DelayProfiles<'_> {
        DelayProfiles(&self.main)
    }

    pub fn release_profiles(&self) -> ReleaseProfiles<'_> {
        ReleaseProfiles(&self.main)
    }

    pub fn movies(&self) -> Movies<'_> {
        Movies(&self.main)
    }

    pub fn movie_files(&self) -> MovieFiles<'_> {
        MovieFiles(&self.main)
    }

    pub fn history(&self) -> History<'_> {
        History(&self.main)
    }

    pub fn blocklist(&self) -> Blocklist<'_> {
        Blocklist(&self.main)
    }

    pub fn pending_releases(&self) -> PendingReleases<'_> {
        PendingReleases(&self.main)
    }

    pub fn media_server_items(&self) -> crate::repo::MediaServerItems<'_> {
        crate::repo::MediaServerItems(&self.main)
    }

    pub fn providers(&self) -> Providers<'_> {
        Providers(&self.main)
    }
}
