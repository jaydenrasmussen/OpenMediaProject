//! Puts the pieces together: application state, the background tasks that
//! keep the library moving, and the HTTP API.

mod api;
mod app;
mod auth;
mod backups;
mod clients;
mod config;
pub mod events;
mod health;
mod ical;
mod lists;
mod logs;
mod manual_import;
mod notify;
pub mod openapi;
mod plugins;
mod releases;
mod tasks;
mod tv_activity;
mod tv_api;

pub use api::router;
pub use app::{App, RealDisk};
pub use config::AppConfig;
pub use lists::sync_once as sync_import_lists;
pub use logs::{DbLogLayer, LogReceiver, channel as log_channel};
pub use notify::dispatch_once as dispatch_notifications;
pub use tasks::{
    process_downloads_once, rescan_movie_once, rss_sync_once, search_movie_once, spawn_scheduler,
};
