//! Puts the pieces together: application state, the background tasks that
//! keep the library moving, and the HTTP API.

mod api;
mod app;
mod auth;
mod backups;
mod browse;
mod cardigann;
mod clients;
mod config;
mod email;
pub mod events;
mod health;
mod host_settings;
mod ical;
mod images;
mod lists;
mod logs;
mod manual_import;
mod media_servers;
mod metadata_files;
mod notify;
pub mod openapi;
mod path_mappings;
mod plugin_client;
mod plugin_install;
mod plugins;
mod releases;
mod tasks;
pub mod tls;
mod tv_activity;
mod tv_api;
pub mod ui_files;
mod vpn;

pub use api::router;
pub use app::{App, RealDisk};
pub use config::{AppConfig, TlsFiles};
pub use host_settings::apply as apply_host_settings;
pub use lists::sync_once as sync_import_lists;
pub use logs::{DbLogLayer, LogReceiver, channel as log_channel};
pub use notify::dispatch_once as dispatch_notifications;
pub use tasks::{
    process_downloads_once, rescan_movie_once, rss_sync_once, search_movie_once, spawn_scheduler,
};
