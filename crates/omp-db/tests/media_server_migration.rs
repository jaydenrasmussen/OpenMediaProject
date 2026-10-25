//! The migration that lets `providers` hold media servers rebuilds the table;
//! what refers to it must survive.

use sqlx::{Row, sqlite::SqliteConnectOptions};
use std::str::FromStr;

#[tokio::test]
async fn existing_providers_and_what_refers_to_them_survive_the_rebuild() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::from_str("sqlite::memory:")
                .unwrap()
                .foreign_keys(true),
        )
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../migrations/main/0001_init.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(
        "INSERT INTO providers (id, kind, name, implementation, settings, priority) VALUES
           (7, 'indexer', 'NZB', 'newznab', '{\"a\":1}', 10),
           (8, 'download_client', 'SAB', 'usenet', '{}', 25);
         INSERT INTO provider_status (provider_id, escalation_level, last_rss_download_url) VALUES (7, 3, 'http://x');
         INSERT INTO remote_path_mappings (client_id, remote_path, local_path) VALUES (8, '/r', '/l'), (NULL, '/a', '/b');",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::raw_sql(include_str!("../migrations/main/0002_media_servers.sql"))
        .execute(&pool)
        .await
        .unwrap();

    let rows = sqlx::query("SELECT id, kind, name, settings, priority FROM providers ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    let got: Vec<(i64, String, String, String, i64)> = rows
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4)))
        .collect();
    assert_eq!(
        got,
        [
            (7, "indexer".into(), "NZB".into(), "{\"a\":1}".into(), 10),
            (8, "download_client".into(), "SAB".into(), "{}".into(), 25),
        ]
    );
    let status: (i64, String) = sqlx::query_as(
        "SELECT escalation_level, last_rss_download_url FROM provider_status WHERE provider_id = 7",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, (3, "http://x".into()));
    let mappings: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM remote_path_mappings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(mappings, 2);

    // The new kind is accepted, and still-unknown kinds are not.
    sqlx::raw_sql("INSERT INTO providers (kind, name, implementation) VALUES ('media_server', 'Plex', 'plex-library')")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        sqlx::raw_sql(
            "INSERT INTO providers (kind, name, implementation) VALUES ('nonsense', 'x', 'y')"
        )
        .execute(&pool)
        .await
        .is_err()
    );
    // Deleting a server removes what it held, and the foreign keys still bite.
    sqlx::raw_sql("INSERT INTO media_server_items (provider_id, item_key, kind, title) VALUES (9, 'k', 'movie', 'M')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("DELETE FROM providers WHERE id = 9")
        .execute(&pool)
        .await
        .unwrap();
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM media_server_items")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
    assert!(
        sqlx::raw_sql("INSERT INTO media_server_items (provider_id, item_key, kind, title) VALUES (999, 'k', 'movie', 'M')")
            .execute(&pool)
            .await
            .is_err()
    );
}
