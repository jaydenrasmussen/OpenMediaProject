use std::{convert::Infallible, env, net::SocketAddr, path::PathBuf};

use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::get,
};
use serde_json::json;
use tower::{ServiceExt, service_fn};
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
    // Static export: unknown *page* paths fall back to the SPA shell. Paths that
    // look like files (have an extension) must 404, or a missing JS/CSS asset
    // would be served as HTML with a 200.
    let shell = ServeFile::new(ui_dir.join("index.html"));
    let ui = ServeDir::new(&ui_dir).fallback(service_fn(move |req: Request<Body>| {
        let shell = shell.clone();
        async move {
            let last = req.uri().path().rsplit('/').next().unwrap_or_default();
            if last.contains('.') {
                Ok(StatusCode::NOT_FOUND.into_response())
            } else {
                Ok::<_, Infallible>(shell.oneshot(req).await.into_response())
            }
        }
    }));
    Router::new()
        .route("/api/v1/health", get(health))
        .fallback_service(ui)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    use super::*;

    fn ui_fixture() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("omp-ui-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("_next")).unwrap();
        std::fs::write(dir.join("index.html"), "<h1>shell</h1>").unwrap();
        std::fs::write(dir.join("_next/app.js"), "console.log(1)").unwrap();
        dir
    }

    async fn status(ui: &Path, path: &str) -> u16 {
        app(ui.to_path_buf())
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
            .as_u16()
    }

    #[tokio::test]
    async fn spa_fallback_only_for_page_paths() {
        let ui = ui_fixture();
        assert_eq!(status(&ui, "/").await, 200);
        assert_eq!(
            status(&ui, "/movies/42").await,
            200,
            "unknown page -> shell"
        );
        assert_eq!(status(&ui, "/_next/app.js").await, 200);
        assert_eq!(
            status(&ui, "/_next/missing.js").await,
            404,
            "missing asset must 404"
        );
    }

    #[tokio::test]
    async fn health_ok() {
        let res = app("nonexistent".into())
            .oneshot(Request::get("/api/v1/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
    }
}
