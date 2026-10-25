//! Remote path mappings: where a download client says its files are versus
//! where this server sees them (clients on another machine or in another
//! container).

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get},
};
use omp_db::repo::ProviderKind;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{ApiError, ApiResult, Shared},
    app::App,
};

pub(crate) fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/remotepathmapping", get(list).post(add))
        .route("/remotepathmapping/{id}", delete(remove))
}

async fn list(State(app): Shared) -> ApiResult {
    let rows = app.db.remote_path_mappings().list().await?;
    Ok(Json(Value::Array(
        rows.iter()
            .map(|m| json!({ "id": m.id, "client_id": m.client_id, "remote_path": m.remote_path, "local_path": m.local_path }))
            .collect(),
    )))
}

#[derive(Deserialize)]
struct NewMapping {
    /// Leave out to apply to every download client.
    client_id: Option<i64>,
    remote_path: String,
    local_path: String,
}

async fn add(
    State(app): Shared,
    Json(req): Json<NewMapping>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let remote = req.remote_path.trim();
    let local = req.local_path.trim();
    if remote.is_empty() {
        return Err(ApiError::bad_request("remote_path is empty"));
    }
    if !std::path::Path::new(local).is_absolute() || !std::path::Path::new(local).is_dir() {
        return Err(ApiError::bad_request(
            "local_path must be an existing folder on this server",
        ));
    }
    if let Some(id) = req.client_id {
        let known = app
            .db
            .providers()
            .list(ProviderKind::DownloadClient)
            .await?
            .iter()
            .any(|p| p.id == id);
        if !known {
            return Err(ApiError::bad_request("unknown client_id"));
        }
    }
    let existing = app.db.remote_path_mappings().list().await?;
    if existing
        .iter()
        .any(|m| m.client_id == req.client_id && m.remote_path == remote)
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "that remote path is already mapped".into(),
        ));
    }
    let id = app
        .db
        .remote_path_mappings()
        .insert(req.client_id, remote, local)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({ "id": id, "client_id": req.client_id, "remote_path": remote, "local_path": local }),
        ),
    ))
}

async fn remove(State(app): Shared, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    app.db.remote_path_mappings().delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
