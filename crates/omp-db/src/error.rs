#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("not found")]
    NotFound,
    /// A unique constraint failed (duplicate name, duplicate TMDB id, ...).
    #[error("already exists: {0}")]
    Conflict(String),
    /// A row refers to something that does not exist, or something that still
    /// refers to this row blocks the delete.
    #[error("constraint failed: {0}")]
    Constraint(String),
    #[error("invalid stored data: {0}")]
    Corrupt(String),
    #[error(transparent)]
    Sql(sqlx::Error),
    #[error("migration failed: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T, E = DbError> = std::result::Result<T, E>;

impl From<sqlx::Error> for DbError {
    fn from(err: sqlx::Error) -> Self {
        if let sqlx::Error::Database(db) = &err {
            let message = db.message().to_owned();
            if db.is_unique_violation() {
                return DbError::Conflict(message);
            }
            if db.is_foreign_key_violation() {
                return DbError::Constraint(message);
            }
        }
        if matches!(err, sqlx::Error::RowNotFound) {
            return DbError::NotFound;
        }
        DbError::Sql(err)
    }
}
