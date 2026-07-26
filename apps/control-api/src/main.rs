use anyhow::Context;
use clap::Parser;
use tracing::info;

mod cli;
mod domain;
mod features;
mod infra;
mod init;
mod state;

use crate::{
    cli::{Cli, Command},
    state::ControlApiState,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = init::config(cli.config_path())?;
    config.init_tracing()?;

    let state = ControlApiState::new(config, cli.config_path());

    if matches!(cli.command, Some(Command::Migrate)) {
        init::migrate(&state).await?;
        return Ok(());
    }

    init::database(&state).await?;
    init::cache(&state).await?;
    spawn_node_health_sweep(state.clone());
    let addr = init::address(&state);
    let app = features::router::router(state.clone()).with_state(state);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind Control API listener on {addr}"))?;

    info!(operation = "control_api.start", %addr, "Control API started");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("Control API server failed")?;
    info!(operation = "control_api.stop", "Control API stopped");

    Ok(())
}

/// Periodically marks Nodes without a recent heartbeat as offline so the
/// admin console and claim decisions reflect real availability.
fn spawn_node_health_sweep(state: ControlApiState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let Some(db) = state.try_database() else {
                continue;
            };
            match domain::nodes::mark_stale_offline(
                db,
                features::api::v1::admin::nodes::HEARTBEAT_STALE_SECONDS,
            )
            .await
            {
                Ok(0) => {}
                Ok(count) => info!(
                    operation = "control_api.node_health_sweep",
                    count, "marked stale nodes offline"
                ),
                Err(error) => tracing::warn!(
                    operation = "control_api.node_health_sweep",
                    %error,
                    "node health sweep failed"
                ),
            }
        }
    });
}

async fn shutdown_signal() {
    #[cfg(unix)]
    let signal = async {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    };
    #[cfg(not(unix))]
    let signal = tokio::signal::ctrl_c();

    if let Err(error) = signal.await {
        tracing::warn!(operation = "control_api.shutdown_signal", %error, "failed to listen for shutdown signal");
    }
}
