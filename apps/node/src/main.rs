use std::sync::{Arc, atomic::AtomicU16};

use anyhow::Context;
use clap::Parser;
use tracing::info;

mod build;
mod capacity;
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
    config.validate()?;

    config.init_tracing()?;

    if config.node.node_token.trim().is_empty() || config.node.node_token == "change-me" {
        anyhow::bail!("node token is not configured; set [node].node_token or GWNODE_NODE_TOKEN");
    }
    if config.node.control_api.trim().is_empty() {
        anyhow::bail!("control api URL is not configured");
    }
    if config.node.capabilities.build {
        build::git::ensure_supported_git().await?;
    }

    let client = ControlApiClient::new(&config.node.control_api, &config.node.node_token)?;
    let resources = if config.node.capabilities.serve {
        Some(capacity::detect(&config.serve)?)
    } else {
        None
    };
    let registration = lifecycle::register(&client, &config, resources).await?;
    let node_id = registration.node_id;
    let gateway_token = registration.gateway_token;

    info!(
        operation = "node.start",
        node_id = %node_id,
        name = %config.node.id,
        "Node started"
    );

    let active_builds = Arc::new(AtomicU16::new(0));
    let heartbeat = lifecycle::spawn_heartbeat(
        client.clone(),
        active_builds.clone(),
        cli.config_path().to_owned(),
        config.config_revision,
    );

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
    let build_loop = config
        .node
        .capabilities
        .build
        .then(|| runtime.clone())
        .flatten()
        .map(|runtime| {
            build::BuildLoop {
                client: client.clone(),
                config: config.clone(),
                runtime,
                active_builds: active_builds.clone(),
            }
            .spawn()
        });

    let mut ssr_manager_for_shutdown = None;
    let (artifact_sync, route_refresh, ssr_reaper, serve_task) = if config.node.capabilities.serve {
        let ssr_manager = Arc::new(serve::ssr::SsrManager::with_client(
            runtime,
            node_id,
            &config,
            client.clone(),
        ));
        ssr_manager_for_shutdown = Some(ssr_manager.clone());
        let ssr_reaper = ssr_manager.clone().spawn_reaper();
        let route_table = Arc::new(serve::routes::RouteTable::default());
        let route_refresh = serve::routes::spawn(
            client.clone(),
            route_table.clone(),
            node_id,
            ssr_manager.clone(),
        );
        let serve_state = Arc::new(serve::ServeState::new(
            client.clone(),
            node_id,
            gateway_token
                .clone()
                .expect("Serve registration requires a gateway token"),
            route_table,
            &config,
            ssr_manager,
        ));
        let artifact_sync = serve::sync::spawn(
            client.clone(),
            std::path::PathBuf::from(&config.serve.artifact_cache_root),
        );
        (
            Some(artifact_sync),
            Some(route_refresh),
            Some(ssr_reaper),
            Some(serve::spawn(serve_state, &config)),
        )
    } else {
        (None, None, None, None)
    };

    wait_for_shutdown().await;

    if let Some(serve_task) = serve_task {
        serve_task.abort();
    }
    if let Some(artifact_sync) = artifact_sync {
        artifact_sync.abort();
    }
    if let Some(route_refresh) = route_refresh {
        route_refresh.abort();
    }
    if let Some(ssr_reaper) = ssr_reaper {
        ssr_reaper.abort();
    }
    if let Some(ssr_manager) = ssr_manager_for_shutdown {
        ssr_manager.release_all().await;
    }

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
