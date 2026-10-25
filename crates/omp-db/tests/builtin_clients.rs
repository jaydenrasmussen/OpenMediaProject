//! A new library starts with both built-in downloaders; one that already has a
//! download client of its own is left alone.

use sqlx::{Row, sqlite::SqliteConnectOptions};
use std::str::FromStr;

async fn pool_with_schema() -> sqlx::SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::from_str("sqlite::memory:")
                .unwrap()
                .foreign_keys(true),
        )
        .await
        .unwrap();
    for sql in [
        include_str!("../migrations/main/0001_init.sql"),
        include_str!("../migrations/main/0002_media_servers.sql"),
    ] {
        sqlx::raw_sql(sql).execute(&pool).await.unwrap();
    }
    pool
}

async fn clients(pool: &sqlx::SqlitePool) -> Vec<(String, bool)> {
    sqlx::query("SELECT name, enabled FROM providers WHERE kind = 'download_client' ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect()
}

#[tokio::test]
async fn a_new_library_has_both_built_in_downloaders_switched_on() {
    let pool = pool_with_schema().await;
    sqlx::raw_sql(include_str!(
        "../migrations/main/0003_builtin_download_clients.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        clients(&pool).await,
        vec![
            ("Built-in torrent".to_owned(), true),
            ("Built-in usenet".to_owned(), true)
        ]
    );
}

#[tokio::test]
async fn a_library_with_its_own_download_client_is_left_alone() {
    let pool = pool_with_schema().await;
    sqlx::raw_sql("INSERT INTO providers (kind, name, implementation, settings) VALUES ('download_client','SAB','usenet','{}')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/main/0003_builtin_download_clients.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(clients(&pool).await, vec![("SAB".to_owned(), true)]);
}
