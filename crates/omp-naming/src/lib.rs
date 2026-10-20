//! Builds file and folder names from patterns like
//! `{Movie Title} ({Release Year}) {Quality Full}`. Ported from Radarr
//! `FileNameBuilder` (GPL-3.0); the pattern language is the same so existing
//! Radarr formats keep working.

mod builder;
mod iso;
mod text;

pub use builder::{
    ColonReplacement, DEFAULT_RELEASE_GROUP, NamingConfig, NamingFile, NamingMedia, NamingMovie,
    build_file_name, build_movie_folder, series_folder,
};
pub use text::{clean_file_name, clean_folder_name, clean_title, clean_title_the, title_the};
