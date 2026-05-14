use std::{env, net::SocketAddr};

use anyhow::Context;
use axum::{Json, Router, routing::get};
use grass_config::{ControlApiConfig, LogFormat};
use serde::Serialize;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config_path = env::var("GWAPI_CONFIG").unwrap_or_else(|_| "config.toml".to_owned());
    let config = ControlApiConfig::load(&config_path)
        .with_context(|| format!("failed to load Control API config from {config_path}"))?;

    init_tracing(&config.log.level, &config.log.format)?;

    let addr = SocketAddr::new(config.server.host, config.server.port);
    let app = Router::new().route("/health", get(health));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind Control API listener on {addr}"))?;

    info!(operation = "control_api.start", %addr, "Control API started");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Control API server failed")?;
    info!(operation = "control_api.stop", "Control API stopped");

    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "control-api",
    })
}

fn init_tracing(level: &str, format: &LogFormat) -> anyhow::Result<()> {
    let filter = EnvFilter::try_new(level).context("invalid tracing filter")?;
    let subscriber = fmt().with_env_filter(filter);

    match format {
        LogFormat::Pretty => subscriber
            .try_init()
            .map_err(|error| anyhow::anyhow!("failed to initialize tracing: {error}"))?,
        LogFormat::Json => subscriber
            .json()
            .try_init()
            .map_err(|error| anyhow::anyhow!("failed to initialize tracing: {error}"))?,
    }

    Ok(())
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::warn!(operation = "control_api.shutdown_signal", %error, "failed to listen for shutdown signal");
    }
}
