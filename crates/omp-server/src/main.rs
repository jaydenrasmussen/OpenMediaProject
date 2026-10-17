use std::{env, net::SocketAddr, path::PathBuf};

use axum::{Json, Router, routing::get};
use serde_json::json;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if env::args().any(|a| a == "--healthcheck") {
        return healthcheck().await;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let addr: SocketAddr = env::var("OMP_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()?;
    let ui_dir = PathBuf::from(env::var("OMP_UI_DIR").unwrap_or_else(|_| "ui/out".into()));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, ui_dir = %ui_dir.display(), "listening");
    axum::serve(listener, app(ui_dir)).await?;
    Ok(())
}

/// Used by the container HEALTHCHECK so the image needs no curl.
async fn healthcheck() -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let port = env::var("OMP_ADDR")
        .ok()
        .and_then(|a| a.rsplit(':').next().map(str::to_owned))
        .unwrap_or_else(|| "8080".into());
    let mut conn = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}")).await?;
    conn.write_all(b"GET /api/v1/health HTTP/1.0\r\n\r\n")
        .await?;
    let mut buf = String::new();
    conn.read_to_string(&mut buf).await?;
    anyhow::ensure!(
        buf.starts_with("HTTP/1.0 200") || buf.starts_with("HTTP/1.1 200"),
        "unhealthy: {buf}"
    );
    Ok(())
}

fn app(ui_dir: PathBuf) -> Router {
    // Static export: unknown paths fall back to the SPA shell.
    let ui = ServeDir::new(&ui_dir).fallback(ServeFile::new(ui_dir.join("index.html")));
    Router::new()
        .route("/api/v1/health", get(health))
        .fallback_service(ui)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn health_ok() {
        let res = app("nonexistent".into())
            .oneshot(Request::get("/api/v1/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
    }
}
