use anyhow::Context;
use clap::Parser;
use grass_config::{LogFormat, NodeConfig};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

mod cli;

use crate::cli::Cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = NodeConfig::load(cli.config_path())
        .with_context(|| format!("failed to load Node config from {}", cli.config_path()))?;

    init_tracing(&config.log.level, &config.log.format)?;

    info!(
        operation = "node.start",
        node_id = %config.node.id,
        "Node started with empty lifecycle"
    );
    wait_for_shutdown().await;
    info!(operation = "node.stop", node_id = %config.node.id, "Node stopped");

    Ok(())
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

async fn wait_for_shutdown() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::warn!(operation = "node.shutdown_signal", %error, "failed to listen for shutdown signal");
    }
}
