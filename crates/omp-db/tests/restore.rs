use omp_db::Db;

#[tokio::test]
async fn a_staged_backup_replaces_the_database_on_the_next_open() {
    let dir = tempfile::tempdir().unwrap();
    let backup = dir.path().join("backup.db");

    let db = Db::open(dir.path()).await.unwrap();
    db.root_folders().add("/media/a").await.unwrap();
    db.backup_to(&backup).await.unwrap();
    db.root_folders().add("/media/b").await.unwrap();

    Db::stage_restore(dir.path(), &backup).await.unwrap();
    // Nothing changes while the old database is open.
    assert_eq!(db.root_folders().list().await.unwrap().len(), 2);
    db.close().await;

    let db = Db::open(dir.path()).await.unwrap();
    let paths: Vec<_> = db
        .root_folders()
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.path)
        .collect();
    assert_eq!(paths, ["/media/a"]);
    assert!(
        dir.path().join("omp.db.before-restore").is_file(),
        "the old database is kept"
    );
    assert!(
        !dir.path().join("omp.db.restore").exists(),
        "applied only once"
    );
}

#[tokio::test]
async fn things_that_are_not_usable_backups_are_refused() {
    let dir = tempfile::tempdir().unwrap();

    let junk = dir.path().join("junk.db");
    std::fs::write(&junk, b"this is not a database").unwrap();
    assert!(Db::verify_backup(&junk).await.is_err());
    assert!(Db::stage_restore(dir.path(), &junk).await.is_err());
    assert!(!dir.path().join("omp.db.restore").exists());

    assert!(
        Db::verify_backup(&dir.path().join("missing.db"))
            .await
            .is_err()
    );

    // A real SQLite file that is not ours.
    let other = dir.path().join("other.db");
    {
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", other.display()))
            .await
            .unwrap();
        sqlx::query("CREATE TABLE t (x)")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }
    assert!(Db::verify_backup(&other).await.is_err());

    // One of ours is fine.
    let ours = tempfile::tempdir().unwrap();
    let db = Db::open(ours.path()).await.unwrap();
    let ok = ours.path().join("ok.db");
    db.backup_to(&ok).await.unwrap();
    Db::verify_backup(&ok).await.unwrap();
}
