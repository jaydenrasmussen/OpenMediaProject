//! Writing `.nfo` files and artwork beside the media, for Kodi and friends.
//! Off unless "write metadata files" is switched on in media management.

use std::path::Path;

use omp_core::movie::{CoverKind, MediaCover};
use omp_db::repo::{MovieRecord, SeriesRecord};
use omp_services::{load_import_config, nfo};

use crate::{app::App, images};

fn first(images: &[MediaCover], kind: CoverKind) -> Option<&str> {
    images
        .iter()
        .find(|i| i.kind == kind)
        .map(|i| i.url.as_str())
}

/// Saves one picture unless it is already there. A failure is logged only:
/// artwork is never worth failing an import over.
async fn picture(app: &App, url: Option<&str>, to: &Path) {
    let Some(url) = url.filter(|_| !to.exists()) else {
        return;
    };
    match images::fetch_cached(app, url).await {
        Ok((bytes, _)) => {
            if let Err(e) = tokio::fs::write(to, bytes).await {
                tracing::warn!(file = %to.display(), "could not save the picture: {e}");
            }
        }
        Err(e) => tracing::warn!(file = %to.display(), "could not fetch the picture: {}", e.1),
    }
}

async fn text(path: &Path, content: &str) {
    if let Err(e) = nfo::write_if_changed(path, content).await {
        tracing::warn!(file = %path.display(), "could not write: {e}");
    }
}

pub async fn write_movie(app: &App, m: &MovieRecord) -> Result<(), String> {
    let folder = Path::new(&m.movie.path);
    let Some(file) = app
        .db
        .movie_files()
        .for_movie(m.movie.id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    let stem = Path::new(&file.relative_path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned());
    let dir = Path::new(&file.relative_path)
        .parent()
        .map(|p| folder.join(p))
        .unwrap_or_else(|| folder.to_path_buf());
    if let Some(stem) = stem {
        text(&dir.join(format!("{stem}.nfo")), &nfo::movie_nfo(m)).await;
    }
    picture(
        app,
        first(&m.metadata.images, CoverKind::Poster),
        &folder.join("poster.jpg"),
    )
    .await;
    picture(
        app,
        first(&m.metadata.images, CoverKind::Fanart),
        &folder.join("fanart.jpg"),
    )
    .await;
    Ok(())
}

pub async fn write_series(app: &App, s: &SeriesRecord) -> Result<(), String> {
    let folder = Path::new(&s.path);
    if !folder.is_dir() {
        return Ok(());
    }
    text(&folder.join("tvshow.nfo"), &nfo::series_nfo(s)).await;
    picture(
        app,
        first(&s.info.images, CoverKind::Poster),
        &folder.join("poster.jpg"),
    )
    .await;
    picture(
        app,
        first(&s.info.images, CoverKind::Fanart),
        &folder.join("fanart.jpg"),
    )
    .await;
    for season in &s.seasons {
        let name = if season.season_number == 0 {
            "season-specials-poster.jpg".to_owned()
        } else {
            format!("season{:02}-poster.jpg", season.season_number)
        };
        picture(
            app,
            first(&season.images, CoverKind::Poster),
            &folder.join(name),
        )
        .await;
    }
    let episodes = app
        .db
        .episodes()
        .for_series(s.id)
        .await
        .map_err(|e| e.to_string())?;
    let files = app
        .db
        .episode_files()
        .for_series(s.id)
        .await
        .map_err(|e| e.to_string())?;
    for f in files {
        let mine: Vec<_> = episodes
            .iter()
            .filter(|e| e.episode_file_id == Some(f.id))
            .cloned()
            .collect();
        if mine.is_empty() {
            continue;
        }
        let path = folder.join(&f.relative_path).with_extension("nfo");
        text(&path, &nfo::episode_nfo(s, &mine)).await;
    }
    Ok(())
}

/// Writes for one movie, one series, or the whole library; does nothing when
/// the setting is off. Returns how many items were visited.
pub async fn write(
    app: &App,
    movie: Option<i64>,
    series: Option<i64>,
    everything: bool,
) -> Result<usize, String> {
    let config = load_import_config(&app.db)
        .await
        .map_err(|e| e.to_string())?;
    if !config.write_metadata {
        return Ok(0);
    }
    let mut visited = 0;
    let movies = match (movie, everything) {
        (Some(id), _) => vec![app.db.movies().get(id).await.map_err(|e| e.to_string())?],
        (None, true) => app.db.movies().list().await.map_err(|e| e.to_string())?,
        _ => Vec::new(),
    };
    for m in &movies {
        write_movie(app, m).await?;
        visited += 1;
    }
    let shows = match (series, everything) {
        (Some(id), _) => vec![app.db.series().get(id).await.map_err(|e| e.to_string())?],
        (None, true) => app.db.series().list().await.map_err(|e| e.to_string())?,
        _ => Vec::new(),
    };
    for s in &shows {
        write_series(app, s).await?;
        visited += 1;
    }
    Ok(visited)
}
