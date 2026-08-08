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
    let mut config = init::config(cli.config_path())?;
    apply_cli(&mut config, &cli);
    config.init_tracing()?;

    let state = ControlApiState::new(config, cli.config_path());

    if matches!(cli.command, Some(Command::Migrate)) {
        init::migrate(&state).await?;
        return Ok(());
    }

    init::database(&state).await?;
    init::storage(&state).await?;
    init::cache(&state).await?;
    spawn_node_health_sweep(state.clone());
    spawn_audit_retention_sweep(state.clone());
    spawn_artifact_retention_sweep(state.clone());
    spawn_storage_migration_sweep(state.clone());
    spawn_screenshot_sweep(state.clone());
    spawn_ssr_lease_sweep(state.clone());
    spawn_node_deletion_sweep(state.clone());
    auto_start_local_node(&state).await;
    let addr = init::address(&state);
    let app = features::router::router(state.clone()).with_state(state.clone());
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
    state.node_manager.shutdown().await;
    info!(operation = "control_api.stop", "Control API stopped");

    Ok(())
}

fn apply_cli(config: &mut infra::config::ControlApiConfig, cli: &Cli) {
    if cli.dev {
        config.development.enabled = true;
    }
}

/// Starts the managed local Node on boot when the platform is ready and the
/// operator opted in via `[node_manager] auto_start_local_node`.
async fn auto_start_local_node(state: &ControlApiState) {
    let auto_start = state
        .config
        .read()
        .unwrap()
        .node_manager
        .auto_start_local_node;
    if !auto_start {
        return;
    }
    let Some(db) = state.try_database() else {
        return;
    };
    if !matches!(init::is_setup_finished(db).await, Ok(true)) {
        return;
    }
    let config_path = state.node_manager.config_path().await;
    if !infra::node_manager::config_file::exists(&config_path) {
        info!(
            operation = "control_api.local_node_autostart_skipped",
            config = %config_path,
            "local node config not generated yet; skipping auto start"
        );
        return;
    }
    match state.node_manager.start().await {
        Ok(_) => info!(
            operation = "control_api.local_node_autostart",
            "local node process started"
        ),
        Err(error) => tracing::warn!(
            operation = "control_api.local_node_autostart_failed",
            %error,
            "failed to auto start local node process"
        ),
    }
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

fn spawn_storage_migration_sweep(state: ControlApiState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            match domain::storage_migrations::sweep(&state).await {
                Ok(result) if result.copied > 0 || result.completed || result.failed => {
                    info!(
                        operation = "control_api.storage_migration_sweep",
                        copied = result.copied,
                        completed = result.completed,
                        failed = result.failed,
                        "storage migration sweep completed"
                    );
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(
                    operation = "control_api.storage_migration_sweep",
                    %error,
                    "storage migration failed"
                ),
            }
        }
    });
}

fn spawn_screenshot_sweep(state: ControlApiState) {
    if state.config.read().unwrap().screenshot.is_none() {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            match domain::screenshots::sweep(&state).await {
                Ok(result)
                    if result.enqueued > 0
                        || result.succeeded > 0
                        || result.retried > 0
                        || result.failed > 0 =>
                {
                    info!(
                        operation = "control_api.screenshot_sweep",
                        enqueued = result.enqueued,
                        succeeded = result.succeeded,
                        retried = result.retried,
                        failed = result.failed,
                        "deployment screenshot sweep completed"
                    );
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(
                    operation = "control_api.screenshot_sweep",
                    %error,
                    "deployment screenshot sweep failed"
                ),
            }
        }
    });
}

/// Removes expired audit events immediately on startup and then once per day.
/// A retention value of zero intentionally keeps events permanently.
fn spawn_audit_retention_sweep(state: ControlApiState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(86_400));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let cutoff = state
                .config
                .read()
                .unwrap()
                .audit
                .retention_cutoff(time::OffsetDateTime::now_utc());
            let Some(cutoff) = cutoff else {
                continue;
            };
            let Some(db) = state.try_database() else {
                continue;
            };
            match domain::audits::prune_events_before(db, cutoff).await {
                Ok(0) => {}
                Ok(count) => info!(
                    operation = "control_api.audit_retention_sweep",
                    count, "removed expired audit events"
                ),
                Err(error) => tracing::warn!(
                    operation = "control_api.audit_retention_sweep",
                    %error,
                    "audit retention sweep failed"
                ),
            }
        }
    });
}

fn spawn_node_deletion_sweep(state: ControlApiState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let Some(db) = state.try_database() else {
                continue;
            };
            if let Err(error) = domain::node_deletions::process_pending_jobs(db).await {
                tracing::warn!(
                    operation = "control_api.node_deletion_sweep",
                    %error,
                    "node deletion sweep failed"
                );
            }
        }
    });
}

/// Applies the administrator-configured artifact retention policy immediately
/// at startup and then once per day. Tombstoned rows are retried even when the
/// original filesystem deletion failed.
fn spawn_artifact_retention_sweep(state: ControlApiState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(86_400));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let Some(db) = state.try_database() else {
                continue;
            };
            let Some(cache) = state.try_cache() else {
                continue;
            };
            let policy = match domain::retention::RetentionPolicy::load(db).await {
                Ok(policy) => policy,
                Err(error) => {
                    tracing::warn!(
                        operation = "control_api.artifact_retention_policy",
                        %error,
                        "failed to load artifact retention policy"
                    );
                    continue;
                }
            };
            let storage = state.storage.clone();
            match domain::retention::sweep(
                db,
                cache,
                &storage,
                policy,
                time::OffsetDateTime::now_utc(),
            )
            .await
            {
                Ok(result) if result.tombstoned > 0 || result.removed > 0 || result.failed > 0 => {
                    info!(
                        operation = "control_api.artifact_retention_sweep",
                        tombstoned = result.tombstoned,
                        removed = result.removed,
                        failed = result.failed,
                        "artifact retention sweep completed"
                    );
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(
                    operation = "control_api.artifact_retention_sweep",
                    %error,
                    "artifact retention sweep failed"
                ),
            }
        }
    });
}

fn spawn_ssr_lease_sweep(state: ControlApiState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let Some(db) = state.try_database() else {
                continue;
            };
            match domain::ssr_leases::release_expired(db, time::OffsetDateTime::now_utc()).await {
                Ok(0) => {}
                Ok(count) => info!(
                    operation = "control_api.ssr_lease_sweep",
                    count, "released expired SSR leases"
                ),
                Err(error) => tracing::warn!(
                    operation = "control_api.ssr_lease_sweep",
                    %error,
                    "SSR lease sweep failed"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_dev_flag_is_applied_after_configuration_load() {
        let mut config = infra::config::ControlApiConfig::default();
        let cli = Cli::try_parse_from(["grass-control-api", "--dev"]).unwrap();

        apply_cli(&mut config, &cli);

        assert!(config.development_enabled());
    }
}
