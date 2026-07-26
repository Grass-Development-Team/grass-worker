use std::sync::{Arc, atomic::AtomicU16};

use anyhow::Context;
use clap::Parser;
use tracing::info;

mod cli;
mod client;
mod config;
mod lifecycle;

use crate::{cli::Cli, client::ControlApiClient, config::NodeConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = NodeConfig::load(cli.config_path())
        .with_context(|| format!("failed to load Node config from {}", cli.config_path()))?;

    config.init_tracing()?;

    if config.node.node_token.trim().is_empty() || config.node.node_token == "change-me" {
        anyhow::bail!("node token is not configured; set [node].node_token or GWNODE_NODE_TOKEN");
    }
    if config.node.control_api.trim().is_empty() {
        anyhow::bail!("control api URL is not configured");
    }

    let client = ControlApiClient::new(&config.node.control_api, &config.node.node_token)?;
    let node_id = lifecycle::register(&client, &config).await?;

    info!(
        operation = "node.start",
        node_id = %node_id,
        name = %config.node.id,
        "Node started"
    );

    let active_builds = Arc::new(AtomicU16::new(0));
    let heartbeat = lifecycle::spawn_heartbeat(client.clone(), active_builds.clone());

    wait_for_shutdown().await;

    heartbeat.abort();
    info!(operation = "node.stop", name = %config.node.id, "Node stopped");

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
