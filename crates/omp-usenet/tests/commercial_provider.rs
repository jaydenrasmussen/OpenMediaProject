//! The NNTP client against a commercial usenet provider. Read-only (such
//! providers refuse posting), and it needs an account, so it only runs when
//! asked:
//! `OMP_NNTP_HOST=… OMP_NNTP_USER=… OMP_NNTP_PASS=… OMP_NNTP_ARTICLE='<id>' \
//!  cargo test -p omp-usenet --test commercial_provider -- --ignored --nocapture`
//! `OMP_NNTP_PORT` defaults to 563 (TLS). `OMP_NNTP_ARTICLE` is the message id of
//! a recent yEnc article; any binary-group article from the last few days works.

use omp_usenet::{NntpConnection, NntpError, ServerConfig, decode_part};

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name}"))
}

fn config(password: String) -> ServerConfig {
    let mut c = ServerConfig::new(
        var("OMP_NNTP_HOST"),
        std::env::var("OMP_NNTP_PORT").map_or(563, |p| p.parse().unwrap()),
    );
    c.tls = true;
    c.username = Some(var("OMP_NNTP_USER"));
    c.password = Some(password);
    c
}

#[tokio::test]
#[ignore = "needs a commercial usenet account (OMP_NNTP_*)"]
async fn the_client_logs_in_and_downloads_from_a_commercial_provider() {
    // A wrong password is refused, not retried forever.
    let bad = NntpConnection::connect(&config("definitely-wrong".into())).await;
    assert!(matches!(bad, Err(NntpError::Auth(_))), "{:?}", bad.err());

    let mut conn = NntpConnection::connect(&config(var("OMP_NNTP_PASS")))
        .await
        .expect("TLS handshake and login");

    let id = var("OMP_NNTP_ARTICLE");
    let id = id.trim_matches(['<', '>']);
    let body = conn.body(id).await.expect("a live article downloads");
    let part = decode_part(&body).expect("the article is valid yEnc");
    assert!(!part.data.is_empty());
    println!("downloaded {} decoded bytes", part.data.len());

    // The same connection reports a missing article and stays usable.
    let missing = conn.body("omp-no-such-article@example.invalid").await;
    assert!(
        matches!(missing, Err(NntpError::ArticleNotFound)),
        "{missing:?}"
    );
    conn.body(id).await.expect("the connection survives a 430");
    conn.quit().await;
}
