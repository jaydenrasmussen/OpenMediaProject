//! Puts the pieces together: application state, the background tasks that
//! keep the library moving, and the HTTP API.

mod api;
mod app;
mod auth;
mod backups;
mod clients;
mod config;
mod health;
mod notify;
mod plugins;
mod releases;
mod tasks;

pub use api::router;
pub use app::{App, RealDisk};
pub use config::AppConfig;
pub use notify::dispatch_once as dispatch_notifications;
pub use tasks::{
    process_downloads_once, rescan_movie_once, rss_sync_once, search_movie_once, spawn_scheduler,
};
