//! Movie metadata and availability. `is_available` is ported from Radarr
//! `Movie.IsAvailable` (GPL-3.0).

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

impl MovieStatus {
    pub fn from_i32(value: i32) -> Option<MovieStatus> {
        Some(match value {
            -1 => MovieStatus::Deleted,
            0 => MovieStatus::Tba,
            1 => MovieStatus::Announced,
            2 => MovieStatus::InCinemas,
            3 => MovieStatus::Released,
            _ => return None,
        })
    }
}

/// How far along a movie is. Order matters: a later status is "more released".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum MovieStatus {
    Deleted = -1,
    /// Only rumours.
    #[default]
    Tba = 0,
    /// Announced, but the cinema date is in the future or unknown.
    Announced = 1,
    /// In cinemas for less than 3 months.
    InCinemas = 2,
    /// Physical or digital release, or in cinemas for more than 3 months.
    Released = 3,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaCover {
    pub kind: CoverKind,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoverKind {
    Poster,
    Fanart,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rating {
    pub votes: u32,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlternativeTitle {
    pub title: String,
    /// ISO 3166-1 country code, if known.
    pub country: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CreditKind {
    Cast,
    Crew,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credit {
    pub person_tmdb_id: i64,
    pub name: String,
    pub kind: CreditKind,
    /// Character for cast, job for crew.
    pub role: String,
    pub order: u32,
    pub image_url: Option<String>,
}

/// Provider data for one movie, independent of the user's library entry.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MovieMetadata {
    pub tmdb_id: i64,
    pub imdb_id: Option<String>,
    pub title: String,
    pub original_title: String,
    /// ISO 639-1 code, as the provider reports it.
    pub original_language: String,
    pub overview: Option<String>,
    pub year: i32,
    /// Minutes; 0 when unknown.
    pub runtime: u32,
    pub status: MovieStatus,
    pub certification: Option<String>,
    pub genres: Vec<String>,
    pub in_cinemas: Option<DateTime<Utc>>,
    pub physical_release: Option<DateTime<Utc>>,
    pub digital_release: Option<DateTime<Utc>>,
    pub collection_tmdb_id: Option<i64>,
    pub collection_title: Option<String>,
    pub website: Option<String>,
    pub studio: Option<String>,
    pub ratings: Option<Rating>,
    pub images: Vec<MediaCover>,
    pub alternative_titles: Vec<AlternativeTitle>,
    pub credits: Vec<Credit>,
    pub popularity: f64,
}

/// When a movie becomes searchable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Always,
    Never,
    From(DateTime<Utc>),
}

impl MovieMetadata {
    /// Date from which a movie with `minimum` availability counts as released.
    ///
    /// `Tba`/`Announced`: always. `InCinemas`: the cinema date when known.
    /// Otherwise: the earlier of the physical and digital releases; with
    /// neither, 90 days after the cinema date; with no dates at all, never.
    pub fn availability(&self, minimum: MovieStatus) -> Availability {
        if matches!(minimum, MovieStatus::Tba | MovieStatus::Announced) {
            return Availability::Always;
        }
        if minimum == MovieStatus::InCinemas
            && let Some(d) = self.in_cinemas
        {
            return Availability::From(d);
        }
        match (self.physical_release, self.digital_release) {
            (Some(p), Some(d)) => Availability::From(p.min(d)),
            (Some(d), None) | (None, Some(d)) => Availability::From(d),
            (None, None) => match self.in_cinemas {
                Some(c) => Availability::From(c + Duration::days(90)),
                None => Availability::Never,
            },
        }
    }

    /// True once the movie may be searched for. `delay_days` is the user's
    /// extra wait after the availability date (not applied to `Always`/`Never`).
    pub fn is_available(&self, minimum: MovieStatus, delay_days: i64, now: DateTime<Utc>) -> bool {
        match self.availability(minimum) {
            Availability::Always => true,
            Availability::Never => false,
            Availability::From(date) => now >= date + Duration::days(delay_days),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn day(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap()
    }

    fn movie(
        cinema: Option<DateTime<Utc>>,
        physical: Option<DateTime<Utc>>,
        digital: Option<DateTime<Utc>>,
    ) -> MovieMetadata {
        MovieMetadata {
            in_cinemas: cinema,
            physical_release: physical,
            digital_release: digital,
            ..Default::default()
        }
    }

    #[test]
    fn announced_is_always_available() {
        let m = movie(None, None, None);
        assert!(m.is_available(MovieStatus::Announced, 30, day(2000, 1, 1)));
        assert!(m.is_available(MovieStatus::Tba, 0, day(2000, 1, 1)));
    }

    #[test]
    fn in_cinemas_uses_the_cinema_date() {
        let m = movie(Some(day(2024, 5, 1)), Some(day(2024, 9, 1)), None);
        assert!(!m.is_available(MovieStatus::InCinemas, 0, day(2024, 4, 30)));
        assert!(m.is_available(MovieStatus::InCinemas, 0, day(2024, 5, 1)));
        // The delay is added to the date.
        assert!(!m.is_available(MovieStatus::InCinemas, 10, day(2024, 5, 10)));
        assert!(m.is_available(MovieStatus::InCinemas, 10, day(2024, 5, 11)));
    }

    #[test]
    fn released_uses_the_earlier_home_release() {
        let m = movie(
            Some(day(2024, 1, 1)),
            Some(day(2024, 6, 1)),
            Some(day(2024, 4, 1)),
        );
        assert_eq!(
            m.availability(MovieStatus::Released),
            Availability::From(day(2024, 4, 1))
        );
        let only_physical = movie(None, Some(day(2024, 6, 1)), None);
        assert_eq!(
            only_physical.availability(MovieStatus::Released),
            Availability::From(day(2024, 6, 1))
        );
    }

    #[test]
    fn released_without_home_release_waits_ninety_days_after_cinemas() {
        let m = movie(Some(day(2024, 1, 1)), None, None);
        assert_eq!(
            m.availability(MovieStatus::Released),
            Availability::From(day(2024, 3, 31))
        );
        // InCinemas with no cinema date falls into the same rule.
        let none = movie(None, None, None);
        assert_eq!(
            none.availability(MovieStatus::InCinemas),
            Availability::Never
        );
        assert!(!none.is_available(MovieStatus::Released, 0, day(2100, 1, 1)));
    }

    #[test]
    fn status_ordering() {
        assert!(MovieStatus::Released > MovieStatus::InCinemas);
        assert!(MovieStatus::Announced > MovieStatus::Tba);
    }
}
