use std::net::SocketAddr;

use anyhow::Context;
use axum::{Json, Router, routing::get};
use clap::Parser;
use serde::Serialize;
use tracing::info;

mod cli;
mod infra;
mod state;

use crate::{
    cli::{Cli, Command},
    infra::{config::ControlApiConfig, database},
};

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let config = ControlApiConfig::load(cli.config_path()).with_context(|| {
        format!(
            "failed to load Control API config from {}",
            cli.config_path()
        )
    })?;
    config.init_tracing()?;

    let database = database::connect(&config.database.url).await?;

    if matches!(cli.command, Some(Command::Migrate)) {
        database::migrate::run(&database).await?;
        info!(
            operation = "control_api.migrate",
            "database migrations completed"
        );
        return Ok(());
    }

    if config.migration.auto_migrate {
        database::migrate::run(&database).await?;
    }

    let state = state::ControlApiState::new(config);
    let addr = SocketAddr::new(state.config.server.host, state.config.server.port);
    let app = Router::new()
        .route("/health", get(health))
        .with_state(state);
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

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::warn!(operation = "control_api.shutdown_signal", %error, "failed to listen for shutdown signal");
    }
}
