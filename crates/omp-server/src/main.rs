use std::env;

use axum::serve::ListenerExt;
use omp_app::{App, AppConfig, router, spawn_scheduler};
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

/// The web UI, compiled in so the binary is the whole app. Debug builds read
/// the folder instead, so UI work needs no rebuild.
#[derive(rust_embed::RustEmbed)]
#[folder = "../../ui/out"]
struct EmbeddedUi;

fn embedded_ui(path: &str) -> Option<std::borrow::Cow<'static, [u8]>> {
    EmbeddedUi::get(path).map(|f| f.data)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if env::args().any(|a| a == "--healthcheck") {
        return healthcheck().await;
    }

    // A .env file is a development convenience; the container has none.
    let _ = dotenvy::dotenv();
    // Console output follows RUST_LOG; the event log in the UI keeps its own filter.
    let (log_layer, log_queue) = omp_app::log_channel();
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        ))
        .with(log_layer)
        .init();

    let mut config = AppConfig::from_env()?;
    // OMP_UI_DIR points at a folder to serve instead of the built-in UI.
    if env::var_os("OMP_UI_DIR").is_none() && EmbeddedUi::get("index.html").is_some() {
        config.embedded_ui = Some(embedded_ui);
    }
    tracing::info!(data_dir = %config.data_dir.display(), ui_dir = %config.ui_dir.display(), "starting");
    if config.tmdb_token.is_none() {
        tracing::warn!(
            "TMDB_READ_TOKEN is not set: movies come from Radarr's metadata server alone and series cannot be added"
        );
    }

    let app = App::open(config).await?;
    let log_writer = log_queue.spawn_writer(app.db.clone());
    let background = spawn_scheduler(app.clone());

    let addr = app.config.addr;
    let service = router(app.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>();
    let listener = tokio::net::TcpListener::bind(addr).await?;
    match &app.config.tls {
        Some(files) => {
            let tls = omp_app::tls::server_config(
                &files.cert.to_string_lossy(),
                &files.key.to_string_lossy(),
            )
            .map_err(|e| anyhow::anyhow!("HTTPS is switched on but unusable: {e}"))?;
            tracing::info!(%addr, "listening (HTTPS)");
            axum::serve(
                omp_app::tls::TlsListener::new(listener, tls)?.tap_io(|_| {}),
                service,
            )
            .with_graceful_shutdown(shutdown_signal(app.clone()))
            .await?;
        }
        None => {
            tracing::info!(%addr, "listening");
            axum::serve(listener, service)
                .with_graceful_shutdown(shutdown_signal(app.clone()))
                .await?;
        }
    }

    tracing::info!("shutting down");
    for task in background {
        task.abort();
    }
    log_writer.abort();
    app.shutdown().await;
    Ok(())
}

async fn shutdown_signal(app: std::sync::Arc<App>) {
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
        () = app.shutdown_requested() => {},
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
    let base = omp_app::ui_files::normalize_base(&env::var("OMP_URL_BASE").unwrap_or_default());
    conn.write_all(format!("GET {base}/api/v1/health HTTP/1.0\r\n\r\n").as_bytes())
        .await?;
    let mut buf = String::new();
    conn.read_to_string(&mut buf).await?;
    anyhow::ensure!(
        buf.starts_with("HTTP/1.0 200") || buf.starts_with("HTTP/1.1 200"),
        "unhealthy: {buf}"
    );
    Ok(())
}
