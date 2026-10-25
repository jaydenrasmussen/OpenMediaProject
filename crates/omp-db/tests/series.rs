use chrono::{NaiveDate, TimeZone, Utc};
use omp_core::{
    language::Language,
    movie::{AlternativeTitle, CoverKind, MediaCover, Rating},
    quality::{Quality, QualityModel},
    release::IndexerFlags,
    tv::{EpisodeInfo, NewItemMonitor, SeriesInfo, SeriesStatus, SeriesType},
};
use omp_db::{
    Db, DbError,
    repo::{EpisodeFileRecord, NewSeries, SeasonRecord, SeriesUpdate},
};
use omp_quality::defaults::default_profiles;

async fn db_with_profile() -> (Db, i32) {
    let db = Db::in_memory().await.unwrap();
    let profile = default_profiles().remove(0);
    let id = db.quality_profiles().insert(&profile).await.unwrap();
    (db, id)
}

fn info(tmdb_id: i64, title: &str) -> SeriesInfo {
    SeriesInfo {
        tmdb_id,
        tvdb_id: Some(tmdb_id + 1000),
        imdb_id: Some("tt0903747".into()),
        title: title.into(),
        original_title: title.into(),
        original_language: "en".into(),
        overview: Some("A teacher.".into()),
        year: 2008,
        status: SeriesStatus::Ended,
        network: Some("AMC".into()),
        runtime: 47,
        genres: vec!["Drama".into()],
        certification: Some("TV-MA".into()),
        first_aired: NaiveDate::from_ymd_opt(2008, 1, 20),
        last_aired: NaiveDate::from_ymd_opt(2013, 9, 29),
        images: vec![MediaCover {
            kind: CoverKind::Poster,
            url: "https://image.example/p.jpg".into(),
        }],
        alternative_titles: vec![AlternativeTitle {
            title: "Braking Bad".into(),
            country: Some("DE".into()),
        }],
        ratings: Some(Rating {
            votes: 10,
            value: 9.0,
        }),
        ..Default::default()
    }
}

fn new_series(info: SeriesInfo, profile: i32) -> NewSeries {
    NewSeries {
        info,
        series_type: SeriesType::Standard,
        monitored: true,
        monitor_new_items: NewItemMonitor::All,
        quality_profile_id: profile,
        season_folder: true,
        use_scene_numbering: false,
        path: "/tv/Breaking Bad (2008)".into(),
        root_folder_path: Some("/tv".into()),
        tags: vec![],
    }
}

fn episode(season: i32, number: i32, aired: (i32, u32, u32)) -> EpisodeInfo {
    let date = NaiveDate::from_ymd_opt(aired.0, aired.1, aired.2).unwrap();
    EpisodeInfo {
        tmdb_id: i64::from(season * 100 + number),
        season_number: season,
        episode_number: number,
        title: format!("Episode {number}"),
        overview: None,
        air_date: Some(date),
        air_date_utc: Some(Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap())),
        runtime: 47,
        finale_type: None,
    }
}

#[tokio::test]
async fn a_series_round_trips_with_its_metadata_and_seasons() {
    let (db, profile) = db_with_profile().await;
    let id = db
        .series()
        .add(&new_series(info(1396, "Breaking Bad"), profile))
        .await
        .unwrap();
    db.series()
        .set_seasons(
            id,
            &[
                SeasonRecord {
                    season_number: 0,
                    monitored: false,
                    title: Some("Specials".into()),
                    images: vec![],
                },
                SeasonRecord {
                    season_number: 1,
                    monitored: true,
                    title: None,
                    images: vec![],
                },
            ],
        )
        .await
        .unwrap();

    let s = db.series().get(id).await.unwrap();
    assert_eq!(
        (s.info.tmdb_id, s.info.title.as_str()),
        (1396, "Breaking Bad")
    );
    assert_eq!(s.info.tvdb_id, Some(2396));
    assert_eq!(s.info.status, SeriesStatus::Ended);
    assert_eq!(s.info.first_aired, NaiveDate::from_ymd_opt(2008, 1, 20));
    assert_eq!(s.info.alternative_titles[0].title, "Braking Bad");
    assert_eq!(s.info.ratings.as_ref().map(|r| r.votes), Some(10));
    assert_eq!(s.seasons.len(), 2);
    assert!(!s.season(0).unwrap().monitored && s.season(1).unwrap().monitored);

    assert_eq!(db.series().list().await.unwrap().len(), 1);
    assert_eq!(
        db.series().find_by_tmdb_id(1396).await.unwrap().unwrap().id,
        id
    );
    assert_eq!(
        db.series().find_by_tvdb_id(2396).await.unwrap().unwrap().id,
        id
    );
    assert!(db.series().find_by_tmdb_id(1).await.unwrap().is_none());
    // Alternative titles find it too, and a duplicate TMDB id is refused.
    assert_eq!(
        db.series()
            .find_by_clean_title("brakingbad")
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        db.series()
            .add(&new_series(info(1396, "Again"), profile))
            .await,
        Err(DbError::Conflict(_))
    ));

    db.series().set_season_monitored(id, 0, true).await.unwrap();
    assert!(
        db.series()
            .get(id)
            .await
            .unwrap()
            .season(0)
            .unwrap()
            .monitored
    );
    assert!(matches!(
        db.series().set_season_monitored(id, 9, true).await,
        Err(DbError::NotFound)
    ));
}

#[tokio::test]
async fn user_choices_and_provider_data_are_updated_separately() {
    let (db, profile) = db_with_profile().await;
    let id = db
        .series()
        .add(&new_series(info(1, "Show"), profile))
        .await
        .unwrap();

    db.series()
        .update(
            id,
            &SeriesUpdate {
                series_type: SeriesType::Anime,
                monitored: false,
                monitor_new_items: NewItemMonitor::None,
                quality_profile_id: profile,
                season_folder: false,
                use_scene_numbering: true,
                path: "/tv/Elsewhere".into(),
                root_folder_path: None,
                tags: vec![3],
            },
        )
        .await
        .unwrap();
    // A refresh changes metadata, not the user's choices.
    let mut fresh = info(1, "Show Renamed");
    fresh.status = SeriesStatus::Continuing;
    db.series().apply_info(id, &fresh).await.unwrap();

    let s = db.series().get(id).await.unwrap();
    assert_eq!(s.info.title, "Show Renamed");
    assert_eq!(s.info.status, SeriesStatus::Continuing);
    assert!(s.last_info_sync.is_some());
    assert_eq!(s.series_type, SeriesType::Anime);
    assert!(!s.monitored && !s.season_folder && s.use_scene_numbering);
    assert_eq!(s.monitor_new_items, NewItemMonitor::None);
    assert_eq!(
        (s.path.as_str(), s.tags.clone()),
        ("/tv/Elsewhere", vec![3])
    );
    assert!(matches!(
        db.series()
            .update(
                999,
                &SeriesUpdate {
                    series_type: SeriesType::Standard,
                    monitored: true,
                    monitor_new_items: NewItemMonitor::All,
                    quality_profile_id: profile,
                    season_folder: true,
                    use_scene_numbering: false,
                    path: String::new(),
                    root_folder_path: None,
                    tags: vec![],
                }
            )
            .await,
        Err(DbError::NotFound)
    ));
}

#[tokio::test]
async fn episodes_files_and_cascades() {
    let (db, profile) = db_with_profile().await;
    let id = db
        .series()
        .add(&new_series(info(1396, "Breaking Bad"), profile))
        .await
        .unwrap();
    let e1 = db
        .episodes()
        .insert(id, &episode(1, 1, (2008, 1, 20)), None, true)
        .await
        .unwrap();
    let e2 = db
        .episodes()
        .insert(id, &episode(1, 2, (2008, 1, 27)), Some(2), false)
        .await
        .unwrap();
    // The same season and number twice is refused.
    assert!(matches!(
        db.episodes()
            .insert(id, &episode(1, 1, (2008, 1, 20)), None, true)
            .await,
        Err(DbError::Conflict(_))
    ));

    let all = db.episodes().for_series(id).await.unwrap();
    assert_eq!(all.len(), 2);
    assert!(all[0].monitored && !all[1].monitored);
    assert_eq!(all[1].absolute_episode_number, Some(2));
    assert!(all[0].has_aired(Utc.with_ymd_and_hms(2008, 2, 1, 0, 0, 0).unwrap()));
    assert!(!all[1].has_aired(Utc.with_ymd_and_hms(2008, 1, 21, 0, 0, 0).unwrap()));
    assert_eq!(db.episodes().find(id, 1, 2).await.unwrap().unwrap().id, e2);
    assert_eq!(
        db.episodes()
            .aired_on(id, NaiveDate::from_ymd_opt(2008, 1, 20).unwrap())
            .await
            .unwrap()
            .len(),
        1
    );

    db.episodes().set_monitored(&[e1, e2], false).await.unwrap();
    assert!(
        db.episodes()
            .for_series(id)
            .await
            .unwrap()
            .iter()
            .all(|e| !e.monitored)
    );

    // Refreshing keeps the flags and the file.
    let mut changed = episode(1, 1, (2008, 1, 20));
    changed.title = "Pilot".into();
    db.episodes().apply_info(e1, &changed, None).await.unwrap();
    assert_eq!(db.episodes().get(e1).await.unwrap().title, "Pilot");
    assert!(!db.episodes().get(e1).await.unwrap().monitored);

    // One file can serve two episodes.
    let file = db
        .episode_files()
        .insert(&EpisodeFileRecord {
            id: 0,
            series_id: id,
            season_number: 1,
            relative_path: "Season 01/Breaking Bad - S01E01-E02.mkv".into(),
            size: 1000,
            date_added: Utc::now(),
            scene_name: Some("Breaking.Bad.S01E01E02.720p".into()),
            release_group: Some("GRP".into()),
            quality: QualityModel {
                quality: Quality::WEBDL_720P,
                ..Default::default()
            },
            languages: vec![Language::ENGLISH],
            media_info: None,
            original_file_path: None,
            indexer_flags: IndexerFlags::default(),
        })
        .await
        .unwrap();
    db.episodes().set_file(&[e1, e2], Some(file)).await.unwrap();
    assert_eq!(db.episodes().for_file(file).await.unwrap().len(), 2);
    assert!(db.episodes().get(e1).await.unwrap().has_file());
    assert_eq!(
        db.episode_files()
            .get(file)
            .await
            .unwrap()
            .release_group
            .as_deref(),
        Some("GRP")
    );

    // Deleting the file leaves the episodes, without a file.
    db.episode_files().delete(file).await.unwrap();
    assert!(!db.episodes().get(e1).await.unwrap().has_file());
    assert_eq!(db.episodes().for_series(id).await.unwrap().len(), 2);

    // Deleting the series takes its episodes and files with it.
    let f2 = db
        .episode_files()
        .insert(&EpisodeFileRecord {
            id: 0,
            series_id: id,
            season_number: 1,
            relative_path: "x.mkv".into(),
            size: 1,
            date_added: Utc::now(),
            scene_name: None,
            release_group: None,
            quality: QualityModel {
                quality: Quality::HDTV_720P,
                ..Default::default()
            },
            languages: vec![],
            media_info: None,
            original_file_path: None,
            indexer_flags: IndexerFlags::default(),
        })
        .await
        .unwrap();
    db.series().delete(id).await.unwrap();
    assert!(db.episodes().for_series(id).await.unwrap().is_empty());
    assert!(matches!(
        db.episode_files().get(f2).await,
        Err(DbError::NotFound)
    ));
    assert!(matches!(
        db.series().delete(id).await,
        Err(DbError::NotFound)
    ));
}
