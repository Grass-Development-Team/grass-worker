use std::sync::{Arc, atomic::AtomicU16};

use anyhow::Context;
use clap::Parser;
use tracing::info;

mod build;
mod cli;
mod client;
mod config;
mod lifecycle;
mod output;
mod runtime;
mod serve;

use crate::{
    cli::{Cli, Command},
    client::ControlApiClient,
    config::NodeConfig,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if let Some(Command::GitProxy { ip, port }) = &cli.command {
        return cli::run_git_proxy(*ip, *port).await;
    }
    let config = NodeConfig::load(cli.config_path())
        .with_context(|| format!("failed to load Node config from {}", cli.config_path()))?;

    config.init_tracing()?;

    if config.node.node_token.trim().is_empty() || config.node.node_token == "change-me" {
        anyhow::bail!("node token is not configured; set [node].node_token or GWNODE_NODE_TOKEN");
    }
    if config.node.control_api.trim().is_empty() {
        anyhow::bail!("control api URL is not configured");
    }
    build::git::ensure_supported_git().await?;

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

    let runtime = match runtime::BuildRuntime::from_config(&config.runtime) {
        Ok(runtime) => Some(Arc::new(runtime)),
        Err(error) => {
            tracing::error!(
                operation = "node.runtime.unavailable",
                %error,
                "container runtime unavailable; builds and SSR serving are disabled until it is fixed"
            );
            None
        }
    };
    let build_loop = runtime.clone().map(|runtime| {
        build::BuildLoop {
            client: client.clone(),
            config: config.clone(),
            runtime,
            active_builds: active_builds.clone(),
        }
        .spawn()
    });

    let ssr_manager = Arc::new(serve::ssr::SsrManager::new(runtime, &config));
    let ssr_reaper = ssr_manager.clone().spawn_reaper();
    let serve_state = Arc::new(serve::ServeState::new(client.clone(), &config, ssr_manager));
    let serve_task = serve::spawn(serve_state, &config);

    wait_for_shutdown().await;

    serve_task.abort();
    ssr_reaper.abort();

    if let Some(build_loop) = build_loop {
        build_loop.abort();
    }
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
