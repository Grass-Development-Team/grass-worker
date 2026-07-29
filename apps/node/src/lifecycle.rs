//! Node registration and heartbeat lifecycle.

use std::time::Duration;

use grass_node_protocol::{
    HeartbeatRequest, NodeCapabilities, NodeResources, RegisterRequest, RegisterResponse,
};
use tracing::{info, warn};

use crate::{client::ControlApiClient, config::NodeConfig};

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const REGISTER_RETRY_DELAY: Duration = Duration::from_secs(5);
const REGISTER_MAX_ATTEMPTS: u32 = 60;

#[derive(Default)]
struct ConfigApplyState {
    applying_revision: Option<u64>,
    error: Option<String>,
}

fn apply_desired_response(
    config_path: &str,
    response: &grass_node_protocol::HeartbeatResponse,
    state: &mut ConfigApplyState,
) {
    let (Some(revision), Some(desired)) = (
        response.desired_config_revision,
        response.desired_config.as_ref(),
    ) else {
        return;
    };
    state.applying_revision = Some(revision);
    state.error = match NodeConfig::persist_desired(config_path, revision, desired) {
        Ok(()) => None,
        Err(error) => Some(error.to_string().chars().take(2_000).collect()),
    };
}

pub fn registration_request(
    config: &NodeConfig,
    resources: Option<NodeResources>,
) -> anyhow::Result<RegisterRequest> {
    config.validate()?;
    if config.node.capabilities.serve && resources.is_none() {
        anyhow::bail!("serve capacity is required when serve capability is enabled");
    }

    Ok(RegisterRequest {
        name: config.node.id.clone(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        capabilities: NodeCapabilities {
            build: config.node.capabilities.build,
            serve: config.node.capabilities.serve,
        },
        build_concurrency: if config.node.capabilities.build {
            config.build.concurrency
        } else {
            0
        },
        serve_base_url: config
            .node
            .capabilities
            .serve
            .then(|| config.serve.public_base_url.clone()),
        resources: config
            .node
            .capabilities
            .serve
            .then_some(resources)
            .flatten(),
        config_revision: config.config_revision,
        effective_config: Some(config.sync_configuration()),
        node_token_configured: config.node_token_configured(),
    })
}

/// Registers the Node, retrying while the Control API is unavailable.
pub async fn register(
    client: &ControlApiClient,
    config: &NodeConfig,
    resources: Option<NodeResources>,
) -> anyhow::Result<RegisterResponse> {
    let request = registration_request(config, resources)?;

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
                if config.node.capabilities.serve
                    && response.gateway_token.as_deref().is_none_or(str::is_empty)
                {
                    anyhow::bail!("control api omitted the serve gateway token");
                }
                return Ok(response);
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
    config_path: String,
    effective_config_revision: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut config_apply = ConfigApplyState::default();
        loop {
            interval.tick().await;
            let request = HeartbeatRequest {
                active_builds: active_builds.load(std::sync::atomic::Ordering::Relaxed),
                effective_config_revision,
                applying_config_revision: config_apply.applying_revision,
                config_apply_error: config_apply.error.clone(),
            };
            match client.heartbeat(&request).await {
                Ok(response) => {
                    apply_desired_response(&config_path, &response, &mut config_apply);
                }
                Err(error) => {
                    warn!(operation = "node.heartbeat.failed", %error, "heartbeat failed");
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::*;
    use grass_node_protocol::NodeResources;

    #[test]
    fn capabilities_require_at_least_one_role() {
        let mut config = NodeConfig::default();
        config.node.capabilities.build = false;
        config.node.capabilities.serve = false;
        assert_eq!(
            config.validate().unwrap_err().to_string(),
            "node must enable build or serve"
        );

        config.node.capabilities.serve = true;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn registration_preserves_serve_only_capability_and_capacity() {
        let mut config = NodeConfig::default();
        config.node.capabilities.build = false;
        let resources = NodeResources {
            cpu_millicores: 800,
            memory_mb: 768,
            disk_mb: 4_096,
            max_deployments: 10,
        };

        let request = registration_request(&config, Some(resources)).unwrap();

        assert!(!request.capabilities.build);
        assert!(request.capabilities.serve);
        assert_eq!(request.build_concurrency, 0);
        assert_eq!(request.resources, Some(resources));
    }

    #[test]
    fn desired_heartbeat_response_is_persisted_as_applying_without_changing_effective_revision() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("grass-node-heartbeat-{unique}.toml"));
        fs::write(
            &path,
            r#"
[node]
id = "node-a"
node_token = "existing-secret-token"
"#,
        )
        .unwrap();
        let mut desired = NodeConfig::load_persisted(&path)
            .unwrap()
            .sync_configuration();
        desired.build.concurrency = 4;
        let response = grass_node_protocol::HeartbeatResponse {
            acknowledged: true,
            desired_config_revision: Some(7),
            desired_config: Some(desired),
        };
        let mut state = ConfigApplyState::default();

        apply_desired_response(path.to_str().unwrap(), &response, &mut state);

        let persisted = NodeConfig::load_persisted(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(state.applying_revision, Some(7));
        assert!(state.error.is_none());
        assert_eq!(persisted.config_revision, 7);
        assert_eq!(persisted.build.concurrency, 4);
        assert_eq!(persisted.node.node_token, "existing-secret-token");
    }
}
