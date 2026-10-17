use omp_core::{
    language::Language,
    movie::{AlternativeTitle, MovieMetadata, MovieStatus},
    quality::Quality,
};
use omp_db::{
    Db,
    repo::{MovieRecord, NewMovie},
};
use omp_library::{Library, LibraryError, MatchType, title_lookup};
use omp_parser::title::parse_movie_title;
use omp_quality::profile::{ProfileItem, QualityProfile};

async fn setup() -> (Db, i32) {
    let db = Db::in_memory().await.unwrap();
    let profile = db
        .quality_profiles()
        .insert(&QualityProfile {
            id: 0,
            name: "HD".into(),
            cutoff: Quality::WEBDL_1080P.id,
            items: vec![ProfileItem::Quality {
                quality: Quality::WEBDL_1080P,
                allowed: true,
            }],
            min_format_score: 0,
            cutoff_format_score: 0,
            min_upgrade_format_score: 1,
            format_items: vec![],
            language: Language::ANY,
            upgrade_allowed: true,
        })
        .await
        .unwrap();
    (db, profile)
}

struct Movie {
    tmdb: i64,
    imdb: Option<&'static str>,
    title: &'static str,
    original: Option<&'static str>,
    year: i32,
    secondary: Option<i32>,
    alternatives: &'static [&'static str],
}

fn movie(tmdb: i64, title: &'static str, year: i32) -> Movie {
    Movie {
        tmdb,
        imdb: None,
        title,
        original: None,
        year,
        secondary: None,
        alternatives: &[],
    }
}

async fn add(db: &Db, profile: i32, m: Movie) -> MovieRecord {
    let metadata = MovieMetadata {
        tmdb_id: m.tmdb,
        imdb_id: m.imdb.map(str::to_owned),
        title: m.title.into(),
        original_title: m.original.unwrap_or_default().into(),
        year: m.year,
        secondary_year: m.secondary,
        status: MovieStatus::Released,
        alternative_titles: m
            .alternatives
            .iter()
            .map(|t| AlternativeTitle {
                title: (*t).into(),
                country: None,
            })
            .collect(),
        ..Default::default()
    };
    let metadata_id = db.movies().upsert_metadata(&metadata).await.unwrap();
    let id = db
        .movies()
        .add(&NewMovie {
            metadata_id,
            monitored: true,
            minimum_availability: MovieStatus::Released,
            quality_profile_id: profile,
            path: format!("/movies/{}", m.title),
            root_folder_path: None,
            tags: vec![],
        })
        .await
        .unwrap();
    db.movies().get(id).await.unwrap()
}

async fn find(
    db: &Db,
    release: &str,
    imdb: Option<&str>,
    tmdb: i64,
) -> Result<Option<(i64, MatchType)>, LibraryError> {
    let parsed = parse_movie_title(release, false).expect("release parses");
    Ok(Library::new(db)
        .match_release(&parsed, imdb, tmdb, None)
        .await?
        .map(|m| (m.movie.movie.tmdb_id, m.match_type)))
}

#[tokio::test]
async fn title_and_year_find_the_movie() {
    let (db, p) = setup().await;
    add(&db, p, movie(603, "The Matrix", 1999)).await;
    add(&db, p, movie(604, "The Matrix Reloaded", 2003)).await;

    assert_eq!(
        find(&db, "The.Matrix.1999.1080p.BluRay.x264-GRP", None, 0)
            .await
            .unwrap(),
        Some((603, MatchType::Title))
    );
    assert_eq!(
        find(
            &db,
            "The.Matrix.Reloaded.2003.1080p.BluRay.x264-GRP",
            None,
            0
        )
        .await
        .unwrap(),
        Some((604, MatchType::Title))
    );
    assert_eq!(
        find(&db, "The.Matrix.2005.1080p.BluRay.x264-GRP", None, 0)
            .await
            .unwrap(),
        None,
        "wrong year"
    );
    assert_eq!(
        find(&db, "Unknown.Film.1999.1080p.BluRay.x264-GRP", None, 0)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn a_release_without_a_year_matches_on_title_alone() {
    let (db, p) = setup().await;
    add(&db, p, movie(603, "The Matrix", 1999)).await;
    // The German-tracker format has no year.
    assert_eq!(
        find(&db, "The.Matrix.German.DL.720p.BluRay.x264-GRP", None, 0)
            .await
            .unwrap(),
        Some((603, MatchType::Title))
    );
}

#[tokio::test]
async fn ids_match_only_when_the_year_agrees() {
    let (db, p) = setup().await;
    let mut m = movie(603, "The Matrix", 1999);
    m.imdb = Some("tt0133093");
    add(&db, p, m).await;

    // The indexer's ids are right.
    assert_eq!(
        find(
            &db,
            "Some.Odd.Name.1999.1080p.BluRay.x264-GRP",
            Some("tt0133093"),
            0
        )
        .await
        .unwrap(),
        Some((603, MatchType::Id)),
        "imdb id beats a title that does not match"
    );
    assert_eq!(
        find(&db, "Some.Odd.Name.1999.1080p.BluRay.x264-GRP", None, 603)
            .await
            .unwrap(),
        Some((603, MatchType::Id))
    );
    // Indexers attach wrong ids; a year that disagrees rejects the id match.
    assert_eq!(
        find(
            &db,
            "Another.Film.2010.1080p.BluRay.x264-GRP",
            Some("tt0133093"),
            603
        )
        .await
        .unwrap(),
        None
    );
    // No year in the release: the id is trusted.
    assert_eq!(
        find(
            &db,
            "Another.Film.German.DL.720p.BluRay.x264-GRP",
            Some("tt0133093"),
            0
        )
        .await
        .unwrap(),
        Some((603, MatchType::Id))
    );
}

#[tokio::test]
async fn original_and_alternative_titles_are_searched() {
    let (db, p) = setup().await;
    let mut amelie = movie(194, "Amelie", 2001);
    amelie.original = Some("Le Fabuleux Destin d'Amélie Poulain");
    add(&db, p, amelie).await;
    let mut other = movie(100, "Shaolin Soccer", 2001);
    other.alternatives = &["Siu lam juk kau", "Kung Fu Soccer"];
    add(&db, p, other).await;

    assert_eq!(
        find(
            &db,
            "Le.Fabuleux.Destin.dAmelie.Poulain.2001.1080p.BluRay.x264-GRP",
            None,
            0
        )
        .await
        .unwrap(),
        Some((194, MatchType::Title)),
        "original title"
    );
    assert_eq!(
        find(&db, "Kung.Fu.Soccer.2001.1080p.BluRay.x264-GRP", None, 0)
            .await
            .unwrap(),
        Some((100, MatchType::Title)),
        "alternative title"
    );
}

#[tokio::test]
async fn roman_and_arabic_numbers_are_interchangeable() {
    let (db, p) = setup().await;
    add(&db, p, movie(1366, "Rocky II", 1979)).await;
    assert_eq!(
        find(&db, "Rocky.2.1979.1080p.BluRay.x264-GRP", None, 0)
            .await
            .unwrap(),
        Some((1366, MatchType::Title))
    );
    assert_eq!(
        find(&db, "Rocky.II.1979.1080p.BluRay.x264-GRP", None, 0)
            .await
            .unwrap(),
        Some((1366, MatchType::Title))
    );
}

#[tokio::test]
async fn the_secondary_year_also_counts() {
    let (db, p) = setup().await;
    let mut m = movie(1, "Old Film", 1990);
    m.secondary = Some(1991);
    add(&db, p, m).await;
    for year in [1990, 1991] {
        assert!(
            find(
                &db,
                &format!("Old.Film.{year}.1080p.BluRay.x264-GRP"),
                None,
                0
            )
            .await
            .unwrap()
            .is_some(),
            "{year}"
        );
    }
    assert!(
        find(&db, "Old.Film.1992.1080p.BluRay.x264-GRP", None, 0)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn two_movies_that_both_fit_are_an_error_not_a_guess() {
    let (db, p) = setup().await;
    // Same title, same year (a remake announced under one name).
    add(&db, p, movie(1, "Twins", 1988)).await;
    add(&db, p, movie(2, "Twins", 1988)).await;
    let err = find(&db, "Twins.1988.1080p.BluRay.x264-GRP", None, 0)
        .await
        .unwrap_err();
    assert!(
        matches!(err, LibraryError::Multiple(ref t) if t.len() == 2),
        "{err:?}"
    );

    // Different years disambiguate.
    add(&db, p, movie(3, "Dune", 1984)).await;
    add(&db, p, movie(4, "Dune", 2021)).await;
    assert_eq!(
        find(&db, "Dune.2021.1080p.BluRay.x264-GRP", None, 0)
            .await
            .unwrap(),
        Some((4, MatchType::Title))
    );
    assert_eq!(
        find(&db, "Dune.1984.1080p.BluRay.x264-GRP", None, 0)
            .await
            .unwrap(),
        Some((3, MatchType::Title))
    );
    // No year: both fit.
    assert!(matches!(
        find(&db, "Dune.German.DL.720p.BluRay.x264-GRP", None, 0).await,
        Err(LibraryError::Multiple(_))
    ));
}

#[tokio::test]
async fn a_search_target_matches_that_movie_even_when_other_titles_exist() {
    let (db, p) = setup().await;
    let target = add(&db, p, movie(1366, "Rocky II", 1979)).await;
    add(&db, p, movie(1367, "Rocky III", 1982)).await;
    let lib = Library::new(&db);

    let parsed = parse_movie_title("Rocky.2.1979.1080p.BluRay.x264-GRP", false).unwrap();
    let m = lib
        .match_release(&parsed, None, 0, Some(&target))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (m.movie.movie.tmdb_id, m.match_type),
        (1366, MatchType::Title),
        "numerals tolerated"
    );

    // A release for a different movie is not matched to the target.
    let other = parse_movie_title("Rocky.III.1982.1080p.BluRay.x264-GRP", false).unwrap();
    assert!(
        lib.match_release(&other, None, 0, Some(&target))
            .await
            .unwrap()
            .is_none(),
        "titles do not fit and no ids agree"
    );

    // A foreign title that the indexer tagged with the right TMDB id.
    let foreign = parse_movie_title("Rocky.Zwei.1979.1080p.BluRay.x264-GRP", false).unwrap();
    let by_id = lib
        .match_release(&foreign, None, 1366, Some(&target))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_id.match_type, MatchType::Id);

    // Right title, wrong year, no ids: not accepted.
    let wrong_year = parse_movie_title("Rocky.II.2005.1080p.BluRay.x264-GRP", false).unwrap();
    assert!(
        lib.match_release(&wrong_year, None, 0, Some(&target))
            .await
            .unwrap()
            .is_none()
    );
}

#[test]
fn title_lookup_produces_roman_variants() {
    let l = title_lookup(&["Rocky 2".to_string()]);
    assert!(l.lookup_titles.contains(&"rocky2".to_string()));
    assert!(l.lookup_titles.contains(&"rockyii".to_string()));
    assert!(l.other_titles.contains(&"rockyii".to_string()));
    // Cleaning lower-cases the title, so the upper-case Roman -> Arabic swap
    // never finds anything; each title still yields three lookup spellings.
    assert_eq!(l.other_titles.len(), 2);
    assert_eq!(l.lookup_titles.len(), 3);
}
