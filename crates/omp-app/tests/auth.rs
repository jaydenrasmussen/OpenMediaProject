#![allow(clippy::cloned_ref_to_slice_refs)] // `&[c.clone()]` reads better than from_ref in these helpers

use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    response::Response,
};
use http_body_util::BodyExt;
use omp_app::{App, AppConfig, router};
use omp_db::Db;
use omp_services::{MediaProbe, NoDiskCheck, ProbeInfo};
use serde_json::{Value, json};
use tower::ServiceExt;

struct NoProbe;

#[async_trait]
impl MediaProbe for NoProbe {
    async fn probe(&self, _: &Path) -> Option<ProbeInfo> {
        None
    }
}

struct Rig {
    router: Router,
    app: Arc<App>,
    _dir: tempfile::TempDir,
}

async fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut config = AppConfig::new(dir.path().join("data"));
    config.ui_dir = dir.path().join("ui");
    let app = App::new(
        config,
        Db::in_memory().await.unwrap(),
        None,
        Arc::new(NoProbe),
        Arc::new(NoDiskCheck),
    )
    .await
    .unwrap();
    Rig {
        router: router(app.clone()),
        app,
        _dir: dir,
    }
}

struct Reply {
    status: StatusCode,
    body: Value,
    cookie: Option<String>,
}

async fn send(
    r: &Rig,
    method: &str,
    path: &str,
    body: Option<Value>,
    headers: &[(&str, String)],
) -> Reply {
    let mut req = Request::builder().method(method).uri(path);
    for (k, v) in headers {
        req = req.header(*k, v);
    }
    let body = match body {
        Some(b) => {
            req = req.header("content-type", "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    parse(
        r.router
            .clone()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap(),
    )
    .await
}

async fn parse(res: Response) -> Reply {
    let status = res.status();
    let cookie = res
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|c| c.split(';').next().unwrap().to_owned());
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    Reply {
        status,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookie,
    }
}

fn cookie(c: &Option<String>) -> (&'static str, String) {
    ("cookie", c.clone().expect("a session cookie was set"))
}

const PASSWORD: &str = "correct horse battery";

async fn set_up(r: &Rig) -> Option<String> {
    let reply = send(
        r,
        "POST",
        "/api/v1/auth/setup",
        Some(json!({ "username": "admin", "password": PASSWORD })),
        &[],
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    reply.cookie
}

#[tokio::test]
async fn nothing_is_open_before_setup_except_health_and_status() {
    let r = rig().await;
    assert_eq!(
        send(&r, "GET", "/api/v1/health", None, &[]).await.status,
        StatusCode::OK
    );

    let status = send(&r, "GET", "/api/v1/auth/status", None, &[]).await;
    assert_eq!(
        (
            status.body["setup_required"].clone(),
            status.body["authenticated"].clone()
        ),
        (json!(true), json!(false))
    );

    for path in [
        "/api/v1/movie",
        "/api/v1/queue",
        "/api/v1/system/status",
        "/api/v1/auth/apikey",
        "/api/v1/indexer",
    ] {
        let reply = send(&r, "GET", path, None, &[]).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(
            reply.body["setup_required"], true,
            "tells the UI to show the setup screen"
        );
    }
    assert_eq!(
        send(
            &r,
            "POST",
            "/api/v1/command",
            Some(json!({"name": "RssSync"})),
            &[]
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&r, "GET", "/api/v1/nope", None, &[]).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn setup_creates_the_one_user_and_logs_them_in() {
    let r = rig().await;
    let weak = send(
        &r,
        "POST",
        "/api/v1/auth/setup",
        Some(json!({ "username": "admin", "password": "short" })),
        &[],
    )
    .await;
    assert_eq!(weak.status, StatusCode::BAD_REQUEST);
    let nameless = send(
        &r,
        "POST",
        "/api/v1/auth/setup",
        Some(json!({ "username": "  ", "password": PASSWORD })),
        &[],
    )
    .await;
    assert_eq!(nameless.status, StatusCode::BAD_REQUEST);

    let cookie_header = set_up(&r).await;
    let c = cookie(&cookie_header);
    assert!(c.1.starts_with("omp_session="));
    assert_eq!(
        send(&r, "GET", "/api/v1/movie", None, &[c.clone()])
            .await
            .status,
        StatusCode::OK,
        "the setup session works"
    );

    let status = send(&r, "GET", "/api/v1/auth/status", None, &[c]).await;
    assert_eq!(
        (
            status.body["setup_required"].clone(),
            status.body["authenticated"].clone()
        ),
        (json!(false), json!(true))
    );

    let again = send(
        &r,
        "POST",
        "/api/v1/auth/setup",
        Some(json!({ "username": "evil", "password": PASSWORD })),
        &[],
    )
    .await;
    assert_eq!(
        again.status,
        StatusCode::CONFLICT,
        "setup is a one-time door"
    );
    let denied = send(&r, "GET", "/api/v1/movie", None, &[]).await;
    assert_eq!(
        (denied.status, denied.body["setup_required"].clone()),
        (StatusCode::UNAUTHORIZED, json!(false))
    );
}

#[tokio::test]
async fn login_logout_and_bad_credentials() {
    let r = rig().await;
    set_up(&r).await;

    let wrong = send(
        &r,
        "POST",
        "/api/v1/auth/login",
        Some(json!({ "username": "admin", "password": "wrong password" })),
        &[],
    )
    .await;
    assert_eq!(
        (wrong.status, wrong.cookie),
        (StatusCode::UNAUTHORIZED, None)
    );
    let unknown = send(
        &r,
        "POST",
        "/api/v1/auth/login",
        Some(json!({ "username": "nobody", "password": PASSWORD })),
        &[],
    )
    .await;
    assert_eq!(unknown.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        unknown.body, wrong.body,
        "no hint about which part was wrong"
    );

    let ok = send(
        &r,
        "POST",
        "/api/v1/auth/login",
        Some(json!({ "username": "ADMIN", "password": PASSWORD })),
        &[],
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "usernames are case-insensitive");
    let c = cookie(&ok.cookie);
    assert_eq!(
        send(&r, "GET", "/api/v1/queue", None, &[c.clone()])
            .await
            .status,
        StatusCode::OK
    );

    let out = send(&r, "POST", "/api/v1/auth/logout", None, &[c.clone()]).await;
    assert_eq!(out.status, StatusCode::OK);
    assert_eq!(
        send(&r, "GET", "/api/v1/queue", None, &[c]).await.status,
        StatusCode::UNAUTHORIZED,
        "the session is dead server-side, not just in the browser"
    );
}

#[tokio::test]
async fn the_cookie_is_http_only_and_the_database_never_holds_the_token() {
    let r = rig().await;
    let res = r
        .router
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/setup")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "username": "admin", "password": PASSWORD }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let raw = res
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(
        raw.contains("HttpOnly") && raw.contains("SameSite=Lax") && raw.contains("Path=/"),
        "{raw}"
    );
    let token = raw
        .split(';')
        .next()
        .unwrap()
        .trim_start_matches("omp_session=")
        .to_owned();
    assert_eq!(token.len(), 64);
    // The stored form is a hash, so the raw token finds nothing.
    assert!(
        r.app
            .db
            .sessions()
            .user_for(&token, chrono::Utc::now())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn the_api_key_works_in_a_header_or_the_query_and_can_be_replaced() {
    let r = rig().await;
    let c = cookie(&set_up(&r).await);
    let key = send(&r, "GET", "/api/v1/auth/apikey", None, &[c.clone()])
        .await
        .body["api_key"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(key.len(), 32);
    assert_eq!(key, r.app.api_key().await.unwrap(), "stable between calls");

    assert_eq!(
        send(
            &r,
            "GET",
            "/api/v1/queue",
            None,
            &[("x-api-key", key.clone())]
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        send(&r, "GET", &format!("/api/v1/queue?apikey={key}"), None, &[])
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        send(
            &r,
            "GET",
            "/api/v1/queue",
            None,
            &[("x-api-key", "0".repeat(32))]
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &r,
            "GET",
            "/api/v1/queue",
            None,
            &[("x-api-key", String::new())]
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );

    let fresh = send(&r, "POST", "/api/v1/auth/apikey/regenerate", None, &[c])
        .await
        .body["api_key"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(fresh, key);
    assert_eq!(
        send(&r, "GET", "/api/v1/queue", None, &[("x-api-key", key)])
            .await
            .status,
        StatusCode::UNAUTHORIZED,
        "the old key stops working"
    );
    assert_eq!(
        send(&r, "GET", "/api/v1/queue", None, &[("x-api-key", fresh)])
            .await
            .status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn the_api_key_works_even_before_setup() {
    let r = rig().await;
    let key = r.app.api_key().await.unwrap();
    assert_eq!(
        send(&r, "GET", "/api/v1/queue", None, &[("x-api-key", key)])
            .await
            .status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn changing_the_password_ends_other_sessions() {
    let r = rig().await;
    let first = cookie(&set_up(&r).await);
    let second = cookie(
        &send(
            &r,
            "POST",
            "/api/v1/auth/login",
            Some(json!({ "username": "admin", "password": PASSWORD })),
            &[],
        )
        .await
        .cookie,
    );

    let wrong = send(
        &r,
        "POST",
        "/api/v1/auth/password",
        Some(
            json!({ "current_password": "nope nope nope", "new_password": "a brand new password" }),
        ),
        &[first.clone()],
    )
    .await;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN);
    let short = send(
        &r,
        "POST",
        "/api/v1/auth/password",
        Some(json!({ "current_password": PASSWORD, "new_password": "tiny" })),
        &[first.clone()],
    )
    .await;
    assert_eq!(short.status, StatusCode::BAD_REQUEST);

    let changed = send(
        &r,
        "POST",
        "/api/v1/auth/password",
        Some(json!({ "current_password": PASSWORD, "new_password": "a brand new password" })),
        &[first.clone()],
    )
    .await;
    assert_eq!(changed.status, StatusCode::OK);
    let fresh = cookie(&changed.cookie);
    assert_eq!(
        send(&r, "GET", "/api/v1/queue", None, &[first])
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&r, "GET", "/api/v1/queue", None, &[second])
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&r, "GET", "/api/v1/queue", None, &[fresh])
            .await
            .status,
        StatusCode::OK,
        "this session carries on"
    );

    let old = send(
        &r,
        "POST",
        "/api/v1/auth/login",
        Some(json!({ "username": "admin", "password": PASSWORD })),
        &[],
    )
    .await;
    assert_eq!(old.status, StatusCode::UNAUTHORIZED);
    let new = send(
        &r,
        "POST",
        "/api/v1/auth/login",
        Some(json!({ "username": "admin", "password": "a brand new password" })),
        &[],
    )
    .await;
    assert_eq!(new.status, StatusCode::OK);
}

#[tokio::test]
async fn the_api_key_cannot_change_the_password() {
    let r = rig().await;
    set_up(&r).await;
    let key = r.app.api_key().await.unwrap();
    let reply = send(
        &r,
        "POST",
        "/api/v1/auth/password",
        Some(json!({ "current_password": PASSWORD, "new_password": "a brand new password" })),
        &[("x-api-key", key)],
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
}
