//! Matching releases to library movies. Ported from Radarr `ParsingService`
//! and the title logic in `MovieService` (GPL-3.0).

mod matching;
mod romans;

pub use matching::{
    Library, LibraryError, MatchType, MovieMatch, TitleLookup, find_by_title, title_lookup,
};
pub use romans::{ARABIC_ROMAN, to_roman};
