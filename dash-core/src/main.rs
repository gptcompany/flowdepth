use std::sync::Arc;

use dash_core::{Collector, config::Config, router};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(error, "dash-core terminated");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let config = Config::from_env().map_err(|e| e.to_string())?;
    tracing::info!(
        bind = %config.bind,
        prometheus = config.prom.label(),
        rektslug = config.rektslug.label(),
        "starting"
    );
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|e| format!("bind {}: {e}", config.bind))?;
    let collector = Arc::new(Collector::new(config).map_err(|e| format!("http client: {e}"))?);
    axum::serve(listener, router(collector))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|e| format!("serve: {e}"))
}
