use omp_core::{language::Language, quality::Quality, release::DownloadProtocol};
use omp_db::{Db, DbError};
use omp_quality::{
    custom_format::{ConditionDef, CustomFormatDef, CustomFormatInput, SpecificationDef},
    delay::{DelayProfile, best_for_tags},
    profile::{ProfileFormatItem, ProfileItem, QualityProfile},
    release_profile::ReleaseProfile,
};

fn profile(name: &str) -> QualityProfile {
    QualityProfile {
        id: 0,
        name: name.into(),
        cutoff: 1001,
        items: vec![
            ProfileItem::Quality {
                quality: Quality::DVD,
                allowed: true,
            },
            ProfileItem::Group {
                id: 1001,
                name: "WEB 1080p".into(),
                qualities: vec![Quality::WEBDL_1080P, Quality::WEBRIP_1080P],
                allowed: true,
            },
        ],
        min_format_score: -10,
        cutoff_format_score: 100,
        min_upgrade_format_score: 5,
        format_items: vec![ProfileFormatItem {
            format_id: 3,
            score: 50,
        }],
        language: Language::ORIGINAL,
        upgrade_allowed: true,
    }
}

#[tokio::test]
async fn config_values() {
    let db = Db::in_memory().await.unwrap();
    let c = db.config();
    assert_eq!(c.get("k").await.unwrap(), None);
    c.set("k", "1").await.unwrap();
    c.set("k", "2").await.unwrap();
    c.set("a", "x").await.unwrap();
    assert_eq!(c.get("k").await.unwrap().as_deref(), Some("2"));
    assert_eq!(
        c.all().await.unwrap(),
        [("a".into(), "x".into()), ("k".into(), "2".into())]
    );
    c.remove("k").await.unwrap();
    assert_eq!(c.get("k").await.unwrap(), None);
}

#[tokio::test]
async fn tags_and_root_folders() {
    let db = Db::in_memory().await.unwrap();
    let a = db.tags().get_or_create("Anime").await.unwrap();
    let again = db.tags().get_or_create("anime").await.unwrap();
    assert_eq!(a.id, again.id, "labels are case-insensitive");
    assert!(matches!(
        db.tags().get_or_create("  ").await,
        Err(DbError::Constraint(_))
    ));
    assert_eq!(db.tags().list().await.unwrap().len(), 1);
    db.tags().delete(a.id).await.unwrap();
    assert!(matches!(
        db.tags().delete(a.id).await,
        Err(DbError::NotFound)
    ));

    let rf = db.root_folders().add("/movies").await.unwrap();
    assert!(matches!(
        db.root_folders().add("/movies").await,
        Err(DbError::Conflict(_))
    ));
    assert_eq!(
        db.root_folders().list().await.unwrap(),
        std::slice::from_ref(&rf)
    );
    db.root_folders().delete(rf.id).await.unwrap();
}

#[tokio::test]
async fn quality_profile_round_trip_and_rules() {
    let db = Db::in_memory().await.unwrap();
    let repo = db.quality_profiles();
    let id = repo.insert(&profile("HD")).await.unwrap();

    let stored = repo.get(id).await.unwrap();
    let mut expected = profile("HD");
    expected.id = id;
    assert_eq!(stored, expected);

    assert!(
        matches!(repo.insert(&profile("hd")).await, Err(DbError::Conflict(_))),
        "names are unique, case-insensitive"
    );

    let mut changed = stored.clone();
    changed.upgrade_allowed = false;
    changed.language = Language::GERMAN;
    repo.update(&changed).await.unwrap();
    assert_eq!(repo.get(id).await.unwrap(), changed);
    assert_eq!(repo.list().await.unwrap().len(), 1);

    repo.delete(id).await.unwrap();
    assert!(matches!(repo.get(id).await, Err(DbError::NotFound)));
    assert!(matches!(repo.delete(id).await, Err(DbError::NotFound)));
}

#[tokio::test]
async fn quality_definitions_default_then_override() {
    let db = Db::in_memory().await.unwrap();
    let repo = db.quality_definitions();
    let defaults = repo.load().await.unwrap();
    assert_eq!(defaults.all().len(), Quality::ALL.len());
    assert_eq!(
        defaults.get(Quality::WEBDL_1080P).unwrap().preferred_size,
        Some(95.0)
    );

    let mut d = defaults.get(Quality::WEBDL_1080P).unwrap().clone();
    d.max_size = Some(60.0);
    d.preferred_size = None;
    repo.save(&d).await.unwrap();
    let loaded = repo.load().await.unwrap();
    assert_eq!(
        loaded.get(Quality::WEBDL_1080P).unwrap().max_size,
        Some(60.0)
    );
    assert_eq!(
        loaded.get(Quality::WEBDL_1080P).unwrap().preferred_size,
        None
    );
    assert_eq!(
        loaded.get(Quality::DVD),
        defaults.get(Quality::DVD),
        "others keep defaults"
    );
}

fn format(name: &str, pattern: &str) -> CustomFormatDef {
    CustomFormatDef {
        id: 0,
        name: name.into(),
        include_when_renaming: false,
        specifications: vec![SpecificationDef {
            name: "title".into(),
            negate: false,
            required: false,
            condition: ConditionDef::ReleaseTitle {
                value: pattern.into(),
            },
        }],
    }
}

#[tokio::test]
async fn custom_formats_validate_and_compile() {
    let db = Db::in_memory().await.unwrap();
    let repo = db.custom_formats();
    let id = repo.insert(&format("Remux", "remux")).await.unwrap();
    assert!(matches!(
        repo.insert(&format("remux", "x")).await,
        Err(DbError::Conflict(_))
    ));
    assert!(
        matches!(
            repo.insert(&format("Bad", "(oops")).await,
            Err(DbError::Constraint(_))
        ),
        "invalid regex is refused"
    );

    let compiled = repo.load_compiled().await.unwrap();
    assert_eq!(compiled.len(), 1);
    let input = CustomFormatInput {
        simple_release_title: Some("Movie 2020 REMUX".into()),
        ..Default::default()
    };
    assert!(compiled[0].matches(&input));
    assert_eq!(compiled[0].id, id);

    let mut def = repo.list().await.unwrap().remove(0);
    def.name = "Remux v2".into();
    repo.update(&def).await.unwrap();
    assert_eq!(repo.list().await.unwrap()[0].name, "Remux v2");
    repo.delete(id).await.unwrap();
    assert!(repo.list().await.unwrap().is_empty());
}

#[tokio::test]
async fn delay_profiles_seed_select_and_protect_the_default() {
    let db = Db::in_memory().await.unwrap();
    let repo = db.delay_profiles();
    repo.seed_default().await.unwrap();
    repo.seed_default().await.unwrap();
    let all = repo.list().await.unwrap();
    assert_eq!(all.len(), 1, "seeding twice adds nothing");
    assert_eq!(all[0].preferred_protocol, DownloadProtocol::Usenet);

    let tagged = DelayProfile {
        id: 0,
        order: 1,
        tags: vec![7],
        torrent_delay: 60,
        preferred_protocol: DownloadProtocol::Torrent,
        ..DelayProfile::default_profile()
    };
    let tagged_id = repo.insert(&tagged).await.unwrap();
    let all = repo.list().await.unwrap();
    assert_eq!(all[0].id, tagged_id, "lowest order first");
    assert_eq!(best_for_tags(&all, &[7]).unwrap().torrent_delay, 60);
    assert_eq!(best_for_tags(&all, &[]).unwrap().torrent_delay, 0);

    let default_id = all[1].id;
    assert!(matches!(
        repo.delete(default_id).await,
        Err(DbError::Constraint(_))
    ));
    repo.delete(tagged_id).await.unwrap();
}

#[tokio::test]
async fn release_profiles_crud() {
    let db = Db::in_memory().await.unwrap();
    let repo = db.release_profiles();
    let mut p = ReleaseProfile {
        id: 0,
        name: "no cams".into(),
        enabled: true,
        required: vec![],
        ignored: vec!["cam".into(), "/ts\\b/i".into()],
        indexer_id: 0,
        tags: vec![1, 2],
    };
    p.id = repo.insert(&p).await.unwrap();
    assert_eq!(repo.list().await.unwrap(), std::slice::from_ref(&p));
    p.enabled = false;
    repo.update(&p).await.unwrap();
    assert!(!repo.list().await.unwrap()[0].enabled);
    repo.delete(p.id).await.unwrap();
    assert!(matches!(repo.delete(p.id).await, Err(DbError::NotFound)));
}

#[tokio::test]
async fn data_survives_reopening_the_files() {
    let dir = std::env::temp_dir().join(format!("omp-db-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    {
        let db = Db::open(&dir).await.unwrap();
        db.config().set("hello", "world").await.unwrap();
        db.quality_profiles()
            .insert(&profile("Kept"))
            .await
            .unwrap();
        db.close().await;
    }
    let db = Db::open(&dir).await.unwrap();
    assert_eq!(
        db.config().get("hello").await.unwrap().as_deref(),
        Some("world")
    );
    assert_eq!(db.quality_profiles().list().await.unwrap()[0].name, "Kept");
    assert!(
        dir.join("omp.db").exists() && dir.join("logs.db").exists(),
        "two separate files"
    );
    db.close().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn foreign_keys_are_enforced() {
    let db = Db::in_memory().await.unwrap();
    let err = sqlx::query(
        "INSERT INTO movies (movie_metadata_id, quality_profile_id, path, added)
         VALUES (999, 999, '/x', '2026-01-01T00:00:00Z')",
    )
    .execute(db.main_pool())
    .await
    .map_err(DbError::from)
    .unwrap_err();
    assert!(matches!(err, DbError::Constraint(_)), "{err:?}");
}
