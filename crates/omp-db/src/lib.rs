//! SQLite storage. Two database files: `omp.db` (library, settings, history)
//! and `logs.db` (log lines), so log churn never blocks library writes.

mod db;
mod error;
pub mod repo;

pub use db::Db;
pub use error::{DbError, Result};
