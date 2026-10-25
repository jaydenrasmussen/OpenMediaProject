//! A disk cache for the pictures the UI shows (posters, fanart), so the
//! browser talks only to this server and a poster is downloaded once.
//! Only hosts on the allow-list are fetched, so this cannot be pointed at
//! the rest of the network.

use std::{path::PathBuf, sync::Arc, time::Duration};

use axum::{
    Router,
    body::Body,
    extract::{Query, State},
    http::{HeaderValue, StatusCode, header},
    response::Response,
    routing::get,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    api::{ApiError, Shared},
    app::App,
};

const MAX_IMAGE: usize = 12 * 1024 * 1024;

pub(crate) fn routes() -> Router<Arc<App>> {
    Router::new().route("/image", get(image))
}

#[derive(Deserialize)]
struct Params {
    url: String,
}

pub(crate) fn cache_dir(app: &App) -> PathBuf {
    app.config.data_dir.join("cache").join("images")
}

fn allowed(app: &App, url: &url::Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some_and(|h| {
            app.config
                .image_hosts
                .iter()
                .any(|a| a.eq_ignore_ascii_case(h))
        })
}

/// The picture's bytes and type, from the cache or fetched now.
pub(crate) async fn fetch_cached(app: &App, raw: &str) -> Result<(Vec<u8>, String), ApiError> {
    let url = url::Url::parse(raw).map_err(|_| ApiError::bad_request("that isn't a URL"))?;
    if !allowed(app, &url) {
        return Err(ApiError::bad_request(
            "pictures from that host are not cached here",
        ));
    }
    let key = format!("{:x}", Sha256::digest(url.as_str().as_bytes()));
    let dir = cache_dir(app);
    let (body_path, type_path) = (dir.join(&key), dir.join(format!("{key}.type")));
    if let (Ok(body), Ok(kind)) = (
        tokio::fs::read(&body_path).await,
        tokio::fs::read_to_string(&type_path).await,
    ) {
        return Ok((body, kind));
    }
    let client = omp_net::client_builder()
        .timeout(Duration::from_secs(20))
        // A redirect could leave the allowed hosts.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut response = client.get(url).send().await.map_err(|e| {
        ApiError(
            StatusCode::BAD_GATEWAY,
            format!("could not fetch it: {}", e.without_url()),
        )
    })?;
    if !response.status().is_success() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            format!("the picture server answered {}", response.status()),
        ));
    }
    let kind = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    if !kind.starts_with("image/") {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "that isn't a picture".into(),
        ));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| {
        ApiError(
            StatusCode::BAD_GATEWAY,
            format!("could not fetch it: {}", e.without_url()),
        )
    })? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_IMAGE {
            return Err(ApiError(
                StatusCode::BAD_GATEWAY,
                "the picture is too large".into(),
            ));
        }
    }
    if tokio::fs::create_dir_all(&dir).await.is_ok() {
        // The type goes last: a body without a type is ignored and fetched again.
        if tokio::fs::write(&body_path, &body).await.is_ok() {
            let _ = tokio::fs::write(&type_path, &kind).await;
        }
    }
    Ok((body, kind))
}

async fn image(State(app): Shared, Query(p): Query<Params>) -> Result<Response, ApiError> {
    let (body, kind) = fetch_cached(&app, &p.url).await?;
    let mut response = Response::new(Body::from(body));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&kind).unwrap_or(HeaderValue::from_static("image/jpeg")),
    );
    // The address of a picture never changes what it shows.
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=2592000, immutable"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}
