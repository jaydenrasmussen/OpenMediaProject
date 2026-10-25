//! Extra files that belong to a video: subtitles and the like. They are found
//! by name (`Movie.2020.1080p.en.srt` goes with `Movie.2020.1080p.mkv`) and
//! follow the video when it is imported or renamed, keeping the part of the
//! name after the video's (`.en.srt`).

use std::path::{Path, PathBuf};

use crate::import::{ImportConfig, transfer};

/// The extensions to carry along, lower case without dots. Empty when the
/// setting is off.
pub(crate) fn wanted_extensions(config: &ImportConfig) -> Vec<String> {
    if !config.import_extra_files {
        return Vec::new();
    }
    config
        .extra_file_extensions
        .split(',')
        .map(|e| e.trim().trim_start_matches('.').to_lowercase())
        .filter(|e| !e.is_empty())
        .collect()
}

/// Files in the video's folder named after it, with the rest of each name.
async fn find(video: &Path, extensions: &[String]) -> Vec<(PathBuf, String)> {
    let (Some(dir), Some(stem)) = (
        video.parent(),
        video.file_stem().map(|s| s.to_string_lossy().into_owned()),
    ) else {
        return Vec::new();
    };
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return Vec::new();
    };
    let mut found = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path == video || !entry.file_type().await.is_ok_and(|t| t.is_file()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(rest) = name
            .get(stem.len()..)
            .filter(|_| name.len() > stem.len() && name[..stem.len()].eq_ignore_ascii_case(&stem))
        else {
            continue;
        };
        if !rest.starts_with('.') {
            continue;
        }
        let ext = rest.rsplit('.').next().unwrap_or_default().to_lowercase();
        if extensions.contains(&ext) {
            found.push((path, rest.to_owned()));
        }
    }
    found.sort();
    found
}

fn target(dest_video: &Path, suffix: &str) -> Option<PathBuf> {
    let stem = dest_video.file_stem()?.to_string_lossy().into_owned();
    Some(dest_video.with_file_name(format!("{stem}{suffix}")))
}

/// Brings the extras of `source_video` along to `dest_video`. A failure is
/// logged and skipped: the video is already in place. Returns how many moved.
pub(crate) async fn import(
    config: &ImportConfig,
    source_video: &Path,
    dest_video: &Path,
    copy_only: bool,
) -> usize {
    let extensions = wanted_extensions(config);
    if extensions.is_empty() {
        return 0;
    }
    let mut done = 0;
    for (path, suffix) in find(source_video, &extensions).await {
        let Some(to) = target(dest_video, &suffix) else {
            continue;
        };
        if to.exists() {
            continue;
        }
        match transfer(&path, &to, copy_only, config.copy_using_hardlinks).await {
            Ok(()) => done += 1,
            Err(e) => tracing::warn!(file = %path.display(), "could not bring along: {e}"),
        }
    }
    done
}

/// Renames the extras of `from_video` to match `to_video`, which has already
/// been renamed (they are looked for next to where the video used to be).
pub(crate) async fn rename(config: &ImportConfig, from_video: &Path, to_video: &Path) -> usize {
    let extensions = wanted_extensions(config);
    if extensions.is_empty() {
        return 0;
    }
    let mut done = 0;
    for (path, suffix) in find(from_video, &extensions).await {
        let Some(to) = target(to_video, &suffix) else {
            continue;
        };
        if to.exists() {
            continue;
        }
        match tokio::fs::rename(&path, &to).await {
            Ok(()) => done += 1,
            Err(e) => tracing::warn!(file = %path.display(), "could not rename: {e}"),
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(on: bool) -> ImportConfig {
        ImportConfig {
            import_extra_files: on,
            extra_file_extensions: "srt, .ass".into(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn subtitles_follow_their_video_and_keep_their_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("in");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        for f in [
            "Movie.2020.mkv",
            "Movie.2020.en.srt",
            "Movie.2020.ass",
            "Movie.2020.nfo",
            "Other.srt",
            "Movie.2020.part.txt",
        ] {
            std::fs::write(src.join(f), b"x").unwrap();
        }
        let dest = out.join("Movie (2020).mkv");

        assert_eq!(
            import(&config(false), &src.join("Movie.2020.mkv"), &dest, true).await,
            0,
            "off by default"
        );
        assert_eq!(
            import(&config(true), &src.join("Movie.2020.mkv"), &dest, false).await,
            2
        );
        let mut names: Vec<_> = std::fs::read_dir(&out)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["Movie (2020).ass", "Movie (2020).en.srt"]);
        assert!(
            src.join("Other.srt").exists() && src.join("Movie.2020.nfo").exists(),
            "unrelated files stay"
        );

        // Renaming the video renames them too.
        let renamed = out.join("Renamed.mkv");
        assert_eq!(rename(&config(true), &dest, &renamed).await, 2);
        assert!(out.join("Renamed.en.srt").exists() && out.join("Renamed.ass").exists());
        assert_eq!(wanted_extensions(&config(true)), ["srt", "ass"]);
    }
}
