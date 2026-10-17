use std::{path::Path, str::FromStr, time::Duration};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

use crate::{
    Result,
    repo::{
        Config, CustomFormats, DelayProfiles, QualityDefinitionsRepo, QualityProfiles,
        ReleaseProfiles, RootFolders, Tags,
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

    pub fn config(&self) -> Config<'_> {
        Config(&self.main)
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
}
