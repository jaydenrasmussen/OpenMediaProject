//! Building custom-format inputs for the different things we score: a new
//! release, the file on disk, and a history event. Ported from Radarr
//! `CustomFormatCalculationService` (GPL-3.0).

use omp_core::{
    language::Language,
    release::{IndexerFlags, ReleaseInfo},
};
use omp_db::repo::{HistoryRecord, MovieFileRecord, MovieRecord};
use omp_parser::title::{ParsedMovieInfo, parse_movie_title, simplify_release_title};
use omp_quality::custom_format::{CustomFormat, CustomFormatInput};

fn original_language(movie: &MovieRecord) -> Option<Language> {
    Language::from_iso639_1(&movie.metadata.original_language)
}

/// The final path component, accepting both separators.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// For a release being considered.
pub fn input_for_release(
    parsed: &ParsedMovieInfo,
    release: &ReleaseInfo,
    movie: &MovieRecord,
) -> CustomFormatInput {
    CustomFormatInput {
        quality: Some(parsed.quality.quality),
        edition: Some(parsed.edition.clone()),
        release_group: parsed.release_group.clone(),
        simple_release_title: Some(parsed.simple_release_title.clone()),
        filename: None,
        parsed_year: parsed.year,
        movie_year: Some(movie.movie.year),
        movie_original_language: original_language(movie),
        size: release.size,
        indexer_flags: release.indexer_flags,
        languages: parsed.languages.clone(),
    }
}

/// For the file we already have. The release title is the scene name, else
/// the name it was downloaded as, else the file's own name.
pub fn input_for_file(file: &MovieFileRecord, movie: &MovieRecord) -> CustomFormatInput {
    let release_title = file
        .scene_name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            file.original_file_path
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(file_name)
        })
        .unwrap_or_else(|| file_name(&file.relative_path));
    CustomFormatInput {
        quality: Some(file.quality.quality),
        edition: file.edition.clone(),
        release_group: file.release_group.clone(),
        simple_release_title: Some(simplify_release_title(release_title)),
        filename: Some(file_name(&file.relative_path).to_owned()),
        parsed_year: 0,
        movie_year: Some(movie.movie.year),
        movie_original_language: original_language(movie),
        size: file.size,
        indexer_flags: file.indexer_flags,
        languages: file.languages.clone(),
    }
}

/// For a past grab or import, from what the history row recorded.
pub fn input_for_history(h: &HistoryRecord, movie: &MovieRecord) -> CustomFormatInput {
    let parsed = parse_movie_title(&h.source_title, false);
    let size = h.data.get("size").and_then(|s| s.parse().ok()).unwrap_or(0);
    let flags = h
        .data
        .get("indexer_flags")
        .and_then(|s| s.parse().ok())
        .map_or(IndexerFlags::NONE, IndexerFlags);
    CustomFormatInput {
        quality: Some(h.quality.quality),
        edition: parsed.as_ref().map(|p| p.edition.clone()),
        release_group: parsed.as_ref().and_then(|p| p.release_group.clone()),
        simple_release_title: Some(parsed.as_ref().map_or_else(
            || simplify_release_title(&h.source_title),
            |p| p.simple_release_title.clone(),
        )),
        filename: None,
        parsed_year: 0,
        movie_year: Some(movie.movie.year),
        movie_original_language: original_language(movie),
        size,
        indexer_flags: flags,
        languages: h.languages.clone(),
    }
}

/// Ids of the formats that match, ready for scoring against a profile.
pub fn matching_ids(formats: &[CustomFormat], input: &CustomFormatInput) -> Vec<i32> {
    omp_quality::custom_format::calculate(formats, input)
        .into_iter()
        .map(|f| f.id)
        .collect()
}

/// For a file about to be imported, before it has a database row.
#[allow(clippy::too_many_arguments)]
pub fn input_for_candidate(
    quality: omp_core::quality::Quality,
    edition: &str,
    release_group: Option<&str>,
    scene_name: &str,
    path: &std::path::Path,
    size: u64,
    languages: &[omp_core::language::Language],
    movie: &MovieRecord,
) -> CustomFormatInput {
    let filename = path.file_name().map(|n| n.to_string_lossy().into_owned());
    CustomFormatInput {
        quality: Some(quality),
        edition: Some(edition.to_owned()),
        release_group: release_group.map(str::to_owned),
        simple_release_title: Some(scene_name.to_owned()).filter(|s| !s.is_empty()),
        filename,
        parsed_year: 0,
        movie_year: Some(movie.movie.year),
        movie_original_language: original_language(movie),
        size,
        indexer_flags: omp_core::release::IndexerFlags(0),
        languages: languages.to_vec(),
    }
}
