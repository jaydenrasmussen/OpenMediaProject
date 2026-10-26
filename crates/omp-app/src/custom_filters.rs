//! Custom filters: a saved search/filter for a library view (movies,
//! series, ...), matching Sonarr/Radarr's `customfilter` endpoint. The
//! `filters` payload is opaque to the server; the UI decides what a filter
//! row means for the view it is scoped to.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use omp_db::repo::CustomFilter;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{ApiError, ApiResult, Shared},
    app::App,
};

pub(crate) fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/customfilter", get(list).post(add))
        .route(
            "/customfilter/{id}",
            get(get_one).put(update).delete(remove),
        )
}

fn to_json(f: &CustomFilter) -> Value {
    json!({ "id": f.id, "type": f.filter_type, "label": f.label, "filters": f.filters })
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(rename = "type")]
    filter_type: Option<String>,
}

async fn list(State(app): Shared, Query(q): Query<ListQuery>) -> ApiResult {
    let rows = app
        .db
        .custom_filters()
        .list(q.filter_type.as_deref())
        .await?;
    Ok(Json(Value::Array(rows.iter().map(to_json).collect())))
}

async fn get_one(State(app): Shared, Path(id): Path<i64>) -> ApiResult {
    let f = app.db.custom_filters().get(id).await?;
    Ok(Json(to_json(&f)))
}

#[derive(Deserialize)]
struct FilterBody {
    #[serde(rename = "type")]
    filter_type: String,
    label: String,
    #[serde(default)]
    filters: Vec<Value>,
}

fn validate(b: &FilterBody) -> Result<(), ApiError> {
    if b.filter_type.trim().is_empty() {
        return Err(ApiError::bad_request("type is required"));
    }
    if b.label.trim().is_empty() {
        return Err(ApiError::bad_request("label is required"));
    }
    Ok(())
}

async fn add(
    State(app): Shared,
    Json(body): Json<FilterBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    validate(&body)?;
    let f = CustomFilter {
        id: 0,
        filter_type: body.filter_type,
        label: body.label,
        filters: body.filters,
    };
    let id = app.db.custom_filters().insert(&f).await?;
    Ok((
        StatusCode::CREATED,
        Json(to_json(&CustomFilter { id, ..f })),
    ))
}

async fn update(
    State(app): Shared,
    Path(id): Path<i64>,
    Json(body): Json<FilterBody>,
) -> ApiResult {
    validate(&body)?;
    let f = CustomFilter {
        id,
        filter_type: body.filter_type,
        label: body.label,
        filters: body.filters,
    };
    app.db.custom_filters().update(&f).await?;
    Ok(Json(to_json(&f)))
}

async fn remove(State(app): Shared, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    app.db.custom_filters().delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
