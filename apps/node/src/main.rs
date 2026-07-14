use anyhow::Context;
use clap::Parser;
use tracing::info;

mod cli;
mod config;

use crate::{cli::Cli, config::NodeConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = NodeConfig::load(cli.config_path())
        .with_context(|| format!("failed to load Node config from {}", cli.config_path()))?;

    config.init_tracing()?;

    info!(
        operation = "node.start",
        node_id = %config.node.id,
        "Node started with empty lifecycle"
    );
    wait_for_shutdown().await;
    info!(operation = "node.stop", node_id = %config.node.id, "Node stopped");

    Ok(())
}

async fn wait_for_shutdown() {
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
        tracing::warn!(operation = "node.shutdown_signal", %error, "failed to listen for shutdown signal");
    }
}
