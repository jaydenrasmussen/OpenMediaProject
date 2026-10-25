//! File extension knowledge. Ported from Radarr `MediaFileExtensions` and
//! `FileExtensions` (GPL-3.0).

use std::sync::LazyLock;

use omp_core::quality::Quality;

use crate::re::Re;

/// Default quality for a file extension, used when the name says nothing.
pub fn quality_for_extension(extension: &str) -> Quality {
    match extension.to_ascii_lowercase().as_str() {
        ".m4v" | ".3gp" | ".nsv" | ".ty" | ".strm" | ".rm" | ".rmvb" | ".m3u" | ".ifo" | ".mov"
        | ".qt" | ".divx" | ".xvid" | ".bivx" | ".nrg" | ".pva" | ".wmv" | ".asf" | ".asx"
        | ".ogm" | ".ogv" | ".m2v" | ".avi" | ".bin" | ".dat" | ".dvr-ms" | ".mpg" | ".mpeg"
        | ".mp4" | ".avc" | ".vp3" | ".svq3" | ".nuv" | ".viv" | ".dv" | ".fli" | ".flv"
        | ".wpl" | ".ts" | ".wtv" => Quality::SDTV,
        ".img" | ".iso" | ".vob" => Quality::DVD,
        ".mkv" | ".mk3d" => Quality::WEBDL_720P,
        ".m2ts" => Quality::BLURAY_720P,
        _ => Quality::UNKNOWN,
    }
}

/// Every extension we treat as media (`.webm` is known but maps to Unknown).
pub fn is_media_extension(extension: &str) -> bool {
    let ext = extension.to_ascii_lowercase();
    ext == ".webm" || quality_for_extension(&ext) != Quality::UNKNOWN
}

fn is_usenet_extension(extension: &str) -> bool {
    matches!(extension.to_ascii_lowercase().as_str(), ".par2" | ".nzb")
}

static FILE_EXTENSION: LazyLock<Re> = LazyLock::new(|| Re::ci(r"\.[a-z0-9]{2,4}$"));

/// Strips a trailing media or usenet extension; leaves anything else alone,
/// so "Movie.2018.720p.x264-GRP" does not lose "x264-GRP"-like tails.
pub fn remove_file_extension(title: &str) -> String {
    FILE_EXTENSION.replace_with(title, |c| {
        let ext = c.whole_str();
        if is_media_extension(ext) || is_usenet_extension(ext) {
            String::new()
        } else {
            ext.to_owned()
        }
    })
}

/// File extension including the dot, like .NET `Path.GetExtension`.
pub fn path_extension(name: &str) -> &str {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    match file.rfind('.') {
        Some(i) if i + 1 < file.len() => &file[i..],
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_only_known_extensions() {
        assert_eq!(remove_file_extension("Movie.2018.mkv"), "Movie.2018");
        assert_eq!(remove_file_extension("Movie.2018.NZB"), "Movie.2018");
        assert_eq!(remove_file_extension("Movie.2018.x264"), "Movie.2018.x264");
        assert_eq!(
            remove_file_extension("Movie.2018.1080p"),
            "Movie.2018.1080p"
        );
    }

    #[test]
    fn path_extension_matches_dotnet() {
        assert_eq!(path_extension("a/b/Movie.mkv"), ".mkv");
        assert_eq!(path_extension("Movie"), "");
        assert_eq!(path_extension("Movie."), "");
        assert_eq!(path_extension(r"C:\x.y\Movie"), "");
    }
}
