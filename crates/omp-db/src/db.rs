use std::{path::Path, str::FromStr, time::Duration};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

use crate::{
    Result,
    repo::{
        Blocklist, Config, CustomFormats, DelayProfiles, History, ImportExclusions, Logs,
        MovieFiles, Movies, PendingReleases, Providers, QualityDefinitionsRepo, QualityProfiles,
        ReleaseProfiles, RootFolders, Sessions, Tags, Users,
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

impl Db {
    /// Opens (creating if needed) `omp.db` and `logs.db` in `dir` and
    /// applies pending migrations.
    pub async fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| crate::DbError::Corrupt(e.to_string()))?;
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

    pub fn providers(&self) -> Providers<'_> {
        Providers(&self.main)
    }
}
