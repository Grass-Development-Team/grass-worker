//! Node registration and heartbeat lifecycle.

use std::time::Duration;

use grass_node_protocol::{HeartbeatRequest, NodeCapabilities, RegisterRequest};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{client::ControlApiClient, config::NodeConfig};

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const REGISTER_RETRY_DELAY: Duration = Duration::from_secs(5);
const REGISTER_MAX_ATTEMPTS: u32 = 60;

/// Applies the first-stage capability rule: build and serve must both be on.
/// Returns the corrected capabilities and logs a warning when the config
/// tried to disable one.
pub fn corrected_capabilities(config: &NodeConfig) -> NodeCapabilities {
    let requested_build = config.node.capabilities.build;
    let requested_serve = config.node.capabilities.serve;
    if !requested_build || !requested_serve {
        warn!(
            operation = "node.capabilities.corrected",
            build = requested_build,
            serve = requested_serve,
            "first-stage nodes must build and serve; enabling both capabilities"
        );
    }
    NodeCapabilities {
        build: true,
        serve: true,
    }
}

/// Registers the Node, retrying while the Control API is unavailable.
pub async fn register(client: &ControlApiClient, config: &NodeConfig) -> anyhow::Result<Uuid> {
    let request = RegisterRequest {
        name: config.node.id.clone(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        capabilities: corrected_capabilities(config),
        build_concurrency: config.build.concurrency,
        serve_base_url: Some(config.serve.public_base_url.clone()),
    };

    let mut attempt = 0;
    loop {
        attempt += 1;
        match client.register(&request).await {
            Ok(response) => {
                info!(
                    operation = "node.registered",
                    node_id = %response.node_id,
                    name = %response.name,
                    "node registered with control api"
                );
                return Ok(response.node_id);
            }
            Err(error) if attempt < REGISTER_MAX_ATTEMPTS => {
                warn!(
                    operation = "node.register.retry",
                    attempt,
                    %error,
                    "registration failed; retrying"
                );
                tokio::time::sleep(REGISTER_RETRY_DELAY).await;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Sends a heartbeat every 30 seconds until the returned handle is aborted.
pub fn spawn_heartbeat(
    client: ControlApiClient,
    active_builds: std::sync::Arc<std::sync::atomic::AtomicU16>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let request = HeartbeatRequest {
                active_builds: active_builds.load(std::sync::atomic::Ordering::Relaxed),
            };
            if let Err(error) = client.heartbeat(&request).await {
                warn!(operation = "node.heartbeat.failed", %error, "heartbeat failed");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_are_forced_to_build_and_serve() {
        let mut config = NodeConfig::default();
        config.node.capabilities.build = false;
        config.node.capabilities.serve = false;
        let corrected = corrected_capabilities(&config);
        assert!(corrected.build);
        assert!(corrected.serve);
    }
}
