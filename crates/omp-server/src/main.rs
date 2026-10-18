use std::env;

use omp_app::{App, AppConfig, router, spawn_scheduler};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if env::args().any(|a| a == "--healthcheck") {
        return healthcheck().await;
    }

    // A .env file is a development convenience; the container has none.
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = AppConfig::from_env()?;
    let addr = config.addr;
    tracing::info!(data_dir = %config.data_dir.display(), ui_dir = %config.ui_dir.display(), "starting");
    if config.tmdb_token.is_none() {
        tracing::warn!("TMDB_READ_TOKEN is not set: movies cannot be added until it is");
    }

    let app = App::open(config).await?;
    let background = spawn_scheduler(app.clone());

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "listening");
    axum::serve(listener, router(app.clone()))
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("shutting down");
    for task in background {
        task.abort();
    }
    app.shutdown().await;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
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
