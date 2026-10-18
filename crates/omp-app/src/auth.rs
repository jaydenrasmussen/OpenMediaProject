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
    if let Some(key) = app.db.config().get(API_KEY_CONFIG).await? {
        return Ok(key);
    }
    let key = random_hex(16);
    app.db.config().set(API_KEY_CONFIG, &key).await?;
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
}

async fn identify(app: &App, headers: &HeaderMap, query: Option<&str>) -> Option<Identity> {
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
    let token = cookie_value(headers, SESSION_COOKIE)?;
    let user = app
        .db
        .sessions()
        .user_for(&hash_token(&token), Utc::now())
        .await
        .ok()??;
    Some(Identity::User(user))
}

/// Rejects requests that are neither logged in nor carrying the API key.
pub async fn require_auth(State(app): State<Arc<App>>, mut req: Request, next: Next) -> Response {
    match identify(&app, req.headers(), req.uri().query()).await {
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

pub async fn status(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    req_uri: axum::extract::OriginalUri,
) -> ApiResult {
    let setup_required = app.db.users().count().await? == 0;
    let authenticated = identify(&app, &headers, req_uri.0.query()).await.is_some();
    Ok(Json(
        json!({ "setup_required": setup_required, "authenticated": authenticated }),
    ))
}

#[derive(Deserialize)]
pub struct Credentials {
    username: String,
    password: String,
}

fn session_cookie(token: &str, max_age_secs: i64) -> String {
    format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age_secs}")
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
        session_cookie(&token, SESSION_DAYS * 86400),
        json!({ "username": user.username }),
    ))
}

pub async fn login(
    State(app): State<Arc<App>>,
    Json(c): Json<Credentials>,
) -> Result<Response, ApiError> {
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
            let token = start_session(&app, user.id).await?;
            Ok(with_cookie(
                session_cookie(&token, SESSION_DAYS * 86400),
                json!({ "username": user.username }),
            ))
        }
        _ => {
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
        session_cookie("", 0),
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
        session_cookie(&token, SESSION_DAYS * 86400),
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
