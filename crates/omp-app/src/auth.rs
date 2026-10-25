//! Who may use the API: a username and password for the browser (a session
//! cookie), or an API key for scripts and other programs.

use std::sync::Arc;

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    api::{ApiError, ApiResult},
    app::App,
};

pub const SESSION_COOKIE: &str = "omp_session";
const SESSION_DAYS: i64 = 30;
const API_KEY_CONFIG: &str = "api_key";
pub const MIN_PASSWORD_LEN: usize = 8;

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("the operating system provides randomness");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Compares without stopping at the first difference.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    diff == 0
}

pub async fn hash_password(password: &str) -> Result<String, ApiError> {
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut salt = [0u8; 16];
        getrandom::fill(&mut salt).expect("the operating system provides randomness");
        let salt = SaltString::encode_b64(&salt).map_err(|e| e.to_string())?;
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| ApiError::internal(e.to_string()))?
    .map_err(ApiError::internal)
}

async fn verify_password(password: &str, hash: &str) -> bool {
    let (password, hash) = (password.to_owned(), hash.to_owned());
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).is_ok_and(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false)
}

/// The API key, created on first use.
pub async fn api_key(app: &App) -> Result<String, omp_db::DbError> {
    api_key_from(&app.db).await
}

pub async fn api_key_from(db: &omp_db::Db) -> Result<String, omp_db::DbError> {
    if let Some(key) = db.config().get(API_KEY_CONFIG).await? {
        return Ok(key);
    }
    let key = random_hex(16);
    db.config().set(API_KEY_CONFIG, &key).await?;
    Ok(key)
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_owned())
}

fn query_param(query: Option<&str>, name: &str) -> Option<String> {
    query?
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Identity {
    User(i64),
    ApiKey,
    /// A machine on the local network, when signing in is switched off for those.
    Local,
}

pub const LOCAL_BYPASS_KEY: &str = "auth_local_bypass";

/// Loopback, private (RFC 1918), link-local and unique-local addresses.
fn is_local(ip: std::net::IpAddr) -> bool {
    match ip.to_canonical() {
        std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// A reverse proxy on this machine makes every client look local, so a
/// request that passed through one never counts.
fn forwarded(headers: &HeaderMap) -> bool {
    [
        "x-forwarded-for",
        "forwarded",
        "x-real-ip",
        "x-forwarded-host",
    ]
    .iter()
    .any(|h| headers.contains_key(*h))
}

async fn identify(
    app: &App,
    headers: &HeaderMap,
    query: Option<&str>,
    remote: Option<std::net::IpAddr>,
) -> Option<Identity> {
    let supplied = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .or_else(|| query_param(query, "apikey"));
    if let Some(supplied) = supplied
        && let Ok(real) = api_key(app).await
        && constant_time_eq(&supplied, &real)
    {
        return Some(Identity::ApiKey);
    }
    if let Some(token) = cookie_value(headers, SESSION_COOKIE)
        && let Ok(Some(user)) = app
            .db
            .sessions()
            .user_for(&hash_token(&token), Utc::now())
            .await
    {
        return Some(Identity::User(user));
    }
    // Local access without signing in, only once an account exists.
    if remote.is_some_and(is_local)
        && !forwarded(headers)
        && app
            .db
            .config()
            .get(LOCAL_BYPASS_KEY)
            .await
            .ok()
            .flatten()
            .as_deref()
            == Some("true")
        && app.db.users().count().await.is_ok_and(|n| n > 0)
    {
        return Some(Identity::Local);
    }
    None
}

/// Rejects requests that are neither logged in nor carrying the API key.
pub async fn require_auth(State(app): State<Arc<App>>, mut req: Request, next: Next) -> Response {
    let remote = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|c| c.0.ip());
    match identify(&app, req.headers(), req.uri().query(), remote).await {
        Some(identity) => {
            req.extensions_mut().insert(identity);
            next.run(req).await
        }
        None => {
            let setup_required = app
                .db
                .users()
                .count()
                .await
                .map(|n| n == 0)
                .unwrap_or(false);
            (
                StatusCode::UNAUTHORIZED,
                Json(
                    json!({ "error": "authentication required", "setup_required": setup_required }),
                ),
            )
                .into_response()
        }
    }
}

// ---- handlers ----------------------------------------------------------------------------

pub async fn status(State(app): State<Arc<App>>, req: Request) -> ApiResult {
    let setup_required = app.db.users().count().await? == 0;
    let remote = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|c| c.0.ip());
    let authenticated = identify(&app, req.headers(), req.uri().query(), remote)
        .await
        .is_some();
    Ok(Json(
        json!({ "setup_required": setup_required, "authenticated": authenticated }),
    ))
}

#[derive(Deserialize)]
pub struct Credentials {
    username: String,
    password: String,
}

/// Behind a TLS-terminating proxy the browser sees HTTPS, and the cookie
/// should then never travel over plain HTTP.
fn is_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("https"))
}

fn session_cookie(token: &str, max_age_secs: i64, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age_secs}{secure}"
    )
}

/// Failed sign-ins per username, and overall, in a sliding window. Keeps a
/// guessing attack slow without locking the real owner out for long.
#[derive(Default)]
pub(crate) struct LoginLimiter {
    failures: std::sync::Mutex<std::collections::HashMap<String, Vec<std::time::Instant>>>,
}

const LIMIT_WINDOW: std::time::Duration = std::time::Duration::from_secs(10 * 60);
const MAX_FAILURES_PER_USER: usize = 5;
const MAX_FAILURES_OVERALL: usize = 50;
const ANY_USER: &str = "\u{0}all";

impl LoginLimiter {
    /// How long until another attempt is allowed, if one is not allowed now.
    pub(crate) fn blocked_for(
        &self,
        username: &str,
        now: std::time::Instant,
    ) -> Option<std::time::Duration> {
        let mut map = self.failures.lock().unwrap();
        let mut wait = |key: &str, max: usize| {
            let recent = map.entry(key.to_owned()).or_default();
            recent.retain(|t| now.duration_since(*t) < LIMIT_WINDOW);
            (recent.len() >= max).then(|| LIMIT_WINDOW - now.duration_since(recent[0]))
        };
        let user = wait(&username.to_lowercase(), MAX_FAILURES_PER_USER);
        let all = wait(ANY_USER, MAX_FAILURES_OVERALL);
        user.into_iter().chain(all).max()
    }

    pub(crate) fn record_failure(&self, username: &str, now: std::time::Instant) {
        let mut map = self.failures.lock().unwrap();
        for key in [username.to_lowercase(), ANY_USER.to_owned()] {
            map.entry(key).or_default().push(now);
        }
    }

    pub(crate) fn clear(&self, username: &str) {
        self.failures
            .lock()
            .unwrap()
            .remove(&username.to_lowercase());
    }
}

async fn start_session(app: &App, user_id: i64) -> Result<String, ApiError> {
    let token = random_hex(32);
    let now = Utc::now();
    app.db
        .sessions()
        .create(
            &hash_token(&token),
            user_id,
            now,
            now + Duration::days(SESSION_DAYS),
        )
        .await?;
    // Old sessions are cleaned up as new ones start.
    let _ = app.db.sessions().purge_expired(now).await;
    Ok(token)
}

fn with_cookie(cookie: String, body: Value) -> Response {
    ([(header::SET_COOKIE, cookie)], Json(body)).into_response()
}

/// Creates the one user. Only works while there is none.
pub async fn setup(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(c): Json<Credentials>,
) -> Result<Response, ApiError> {
    let username = c.username.trim();
    if username.is_empty() {
        return Err(ApiError::bad_request("a username is required"));
    }
    if c.password.chars().count() < MIN_PASSWORD_LEN {
        return Err(ApiError::bad_request(format!(
            "the password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    let hash = hash_password(&c.password).await?;
    let Some(user) = app.db.users().create_first(username, &hash).await? else {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "a user already exists".into(),
        ));
    };
    let token = start_session(&app, user.id).await?;
    Ok(with_cookie(
        session_cookie(&token, SESSION_DAYS * 86400, is_https(&headers)),
        json!({ "username": user.username }),
    ))
}

pub async fn login(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(c): Json<Credentials>,
) -> Result<Response, ApiError> {
    let now = std::time::Instant::now();
    if let Some(wait) = app.login_limiter.blocked_for(c.username.trim(), now) {
        let minutes = wait.as_secs().div_ceil(60).max(1);
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            format!("too many failed attempts; try again in {minutes} minute(s)"),
        ));
    }
    let user = app.db.users().by_username(c.username.trim()).await?;
    // Always do the expensive check so timing does not reveal valid usernames.
    let ok = verify_password(
        &c.password,
        user.as_ref().map_or(&app.dummy_hash, |u| &u.password_hash),
    )
    .await
        && user.is_some();
    match user {
        Some(user) if ok => {
            app.login_limiter.clear(c.username.trim());
            let token = start_session(&app, user.id).await?;
            Ok(with_cookie(
                session_cookie(&token, SESSION_DAYS * 86400, is_https(&headers)),
                json!({ "username": user.username }),
            ))
        }
        _ => {
            app.login_limiter.record_failure(c.username.trim(), now);
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "wrong username or password".into(),
            ))
        }
    }
}

pub async fn logout(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Response, ApiError> {
    if let Some(token) = cookie_value(&headers, SESSION_COOKIE) {
        app.db.sessions().delete(&hash_token(&token)).await?;
    }
    Ok(with_cookie(
        session_cookie("", 0, is_https(&headers)),
        json!({ "status": "logged out" }),
    ))
}

#[derive(Deserialize)]
pub struct ChangePassword {
    current_password: String,
    new_password: String,
}

pub async fn change_password(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    axum::Extension(identity): axum::Extension<Identity>,
    Json(c): Json<ChangePassword>,
) -> Result<Response, ApiError> {
    let Identity::User(user_id) = identity else {
        return Err(ApiError::bad_request(
            "the password can only be changed from a login session",
        ));
    };
    if c.new_password.chars().count() < MIN_PASSWORD_LEN {
        return Err(ApiError::bad_request(format!(
            "the password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    let user = app.db.users().get(user_id).await?;
    if !verify_password(&c.current_password, &user.password_hash).await {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "the current password is wrong".into(),
        ));
    }
    app.db
        .users()
        .set_password_hash(user_id, &hash_password(&c.new_password).await?)
        .await?;
    // Every other login ends; this one carries on.
    app.db.sessions().delete_for_user(user_id).await?;
    let token = start_session(&app, user_id).await?;
    Ok(with_cookie(
        session_cookie(&token, SESSION_DAYS * 86400, is_https(&headers)),
        json!({ "status": "password changed" }),
    ))
}

pub async fn get_api_key(State(app): State<Arc<App>>) -> ApiResult {
    Ok(Json(json!({ "api_key": api_key(&app).await? })))
}

pub async fn regenerate_api_key(State(app): State<Arc<App>>) -> ApiResult {
    let key = random_hex(16);
    app.db.config().set(API_KEY_CONFIG, &key).await?;
    Ok(Json(json!({ "api_key": key })))
}

/// Whether machines on the local network may skip signing in.
pub async fn get_security(State(app): State<Arc<App>>) -> ApiResult {
    let on = app.db.config().get(LOCAL_BYPASS_KEY).await?.as_deref() == Some("true");
    Ok(Json(json!({ "disable_auth_for_local": on })))
}

#[derive(Deserialize)]
pub struct SecuritySettings {
    disable_auth_for_local: bool,
}

pub async fn put_security(
    State(app): State<Arc<App>>,
    Json(s): Json<SecuritySettings>,
) -> ApiResult {
    if s.disable_auth_for_local && app.db.users().count().await? == 0 {
        return Err(ApiError::bad_request("create an account first"));
    }
    app.db
        .config()
        .set(
            LOCAL_BYPASS_KEY,
            if s.disable_auth_for_local {
                "true"
            } else {
                "false"
            },
        )
        .await?;
    get_security(State(app)).await
}

#[cfg(test)]
mod local_tests {
    use super::*;

    #[test]
    fn only_local_network_addresses_count() {
        for ok in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.9",
            "192.168.1.5",
            "169.254.1.1",
            "::1",
            "fd12::1",
            "fe80::1",
            "::ffff:192.168.1.5",
        ] {
            assert!(is_local(ok.parse().unwrap()), "{ok}");
        }
        for no in ["8.8.8.8", "172.32.0.1", "2001:db8::1", "::ffff:8.8.8.8"] {
            assert!(!is_local(no.parse().unwrap()), "{no}");
        }
    }

    #[test]
    fn a_proxy_header_cancels_it() {
        let mut h = HeaderMap::new();
        assert!(!forwarded(&h));
        h.insert("x-forwarded-for", "1.2.3.4".parse().unwrap());
        assert!(forwarded(&h));
    }
}
