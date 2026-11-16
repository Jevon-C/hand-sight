//! Hand Sight server program.
//!
//! Usage: `axonhand-server [settings.toml]`. See `Config` for the settings and the
//! `AXONHAND_*` environment variables that override them.

use std::path::PathBuf;
use std::process::ExitCode;

use axonhand_server::app::{router, Services};
use axonhand_server::config::Config;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            tracing::error!("{message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let file = std::env::args_os().nth(1).map(PathBuf::from);
    let config = Config::load(file.as_deref(), |name| std::env::var(name).ok())
        .map_err(|e| e.to_string())?;
    let services = Services::from_config(&config)?;
    let _worker = services.jobs.start()?;
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|e| format!("cannot listen on {}: {e}", config.bind))?;
    tracing::info!(address = %config.bind, "Hand Sight is running");
    axum::serve(listener, router(&config.static_dir, services))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| format!("server stopped unexpectedly: {e}"))
}

/// Resolves when the user presses Ctrl+C.
async fn shutdown_signal() {
    if tokio::signal::ctrl_c().await.is_err() {
        // If the signal handler cannot be installed, never stop on our own.
        std::future::pending::<()>().await;
    }
    tracing::info!("shutting down");
}
