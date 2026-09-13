use axum::{Extension, Json, extract::State, response::IntoResponse};
use grass_node_protocol::{
    GatewayAuthenticationMode, NodeCapabilities, NodeConfiguration, NodeResources,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::nodes::{self, RegisterNodeParams},
    infra::{
        audit::CreateAuditEventParams,
        database::entity::AuditEventResult,
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegisterRequest {
    name: String,
    version: String,
    capabilities: NodeCapabilities,
    #[serde(default = "default_region")]
    region: String,
    build_concurrency: u16,
    /// Public base URL of the Node serve listener, when known.
    #[serde(default)]
    serve_base_url: Option<String>,
    /// Schedulable Serve capacity. Build-only Nodes omit this field.
    #[serde(default)]
    resources: Option<NodeResources>,
    /// Revision loaded by the running Node process.
    #[serde(default)]
    config_revision: u64,
    /// Effective non-secret configuration loaded by this process.
    #[serde(default)]
    effective_config: Option<NodeConfiguration>,
    /// Whether the running process has a usable token, without exposing it.
    #[serde(default)]
    node_token_configured: bool,
    /// Gateway authentication selected by the running Node.
    #[serde(default)]
    gateway_authentication: GatewayAuthenticationMode,
}

fn default_region() -> String {
    "default".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegisterResponse {
    node_id: Uuid,
    name: String,
    /// Capabilities after server-side correction; the first stage forces
    /// build and serve on.
    capabilities: NodeCapabilities,
    /// Shared credential for authenticated Serve-to-Serve proxying. It is
    /// present only when Serve capability is enabled.
    #[serde(default)]
    gateway_token: Option<String>,
    /// Gateway authentication accepted by the Control API.
    #[serde(default)]
    gateway_authentication: GatewayAuthenticationMode,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route("/nodes/register", axum::routing::post(register))
}

/// POST /api/v1/internal/nodes/register
async fn register(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Json(body): Json<RegisterRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.nodes.register";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;

    validate_registration(&body).map_err(|message| AppError::Validation {
        op: OP,
        message: message.to_owned(),
    })?;
    validate_existing_policy(&body, nodes::gateway_authentication(&node)).map_err(|message| {
        AppError::Validation {
            op: OP,
            message: message.to_owned(),
        }
    })?;
    let region =
        grass_validator::normalize_region(&body.region).map_err(|e| AppError::Validation {
            op: OP,
            message: e.to_string(),
        })?;
    crate::domain::regions::require(db, &region, OP).await?;
    if body
        .effective_config
        .as_ref()
        .is_some_and(|c| c.node.region != region)
    {
        return Err(AppError::Validation {
            op: OP,
            message: "Node region must match its effective configuration".to_owned(),
        });
    }
    let build_enabled = body.capabilities.build;
    let serve_enabled = body.capabilities.serve;

    let name = if body.name.trim().is_empty() {
        node.name.clone()
    } else {
        body.name.trim().to_owned()
    };

    let node = nodes::apply_registration(
        db,
        node,
        RegisterNodeParams {
            name,
            region: grass_validator::normalize_region(&body.region).map_err(|error| {
                AppError::Validation {
                    op: OP,
                    message: format!("region: {error}"),
                }
            })?,
            version: body.version,
            build_enabled: body.capabilities.build,
            serve_enabled: body.capabilities.serve,
            build_concurrency: i32::from(body.build_concurrency),
            base_url: body.serve_base_url,
            resources: body.resources,
            config_revision: body.config_revision,
            effective_config: body.effective_config,
            node_token_configured: body.node_token_configured,
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: None,
            actor_node_id: Some(node.id),
            team_id: None,
            action: "node.registered".to_owned(),
            target_type: "node".to_owned(),
            target_id: Some(node.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "name": node.name }),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    // Inbound policy does not determine outbound credentials: a trusted-network
    // entry still needs a token when its destination requires authentication.
    let gateway_token = serve_enabled
        .then(|| nodes::gateway_token(&state.config.read().unwrap().secrets.secret_key));
    Ok(ok_response(RegisterResponse {
        node_id: node.id,
        name: node.name,
        capabilities: NodeCapabilities {
            build: build_enabled,
            serve: serve_enabled,
        },
        gateway_token,
        gateway_authentication: body.gateway_authentication,
    }))
}

fn validate_registration(body: &RegisterRequest) -> Result<(), &'static str> {
    match &body.effective_config {
        Some(config) if config.security.gateway_authentication != body.gateway_authentication => {
            return Err("gateway authentication must match the effective configuration");
        }
        None if matches!(body.gateway_authentication, GatewayAuthenticationMode::None) => {
            return Err("effective configuration is required for no-token gateways");
        }
        _ => {}
    }
    if grass_validator::normalize_region(&body.region).is_err() {
        return Err("region is invalid");
    }
    if !body.capabilities.build && !body.capabilities.serve {
        return Err("node must enable build or serve");
    }
    if body.capabilities.build && body.build_concurrency == 0 {
        return Err("build concurrency must be positive for a build node");
    }
    if !body.capabilities.build && body.build_concurrency != 0 {
        return Err("build-only settings are not allowed for a non-build node");
    }
    if body.capabilities.serve {
        let Some(base_url) = body.serve_base_url.as_deref() else {
            return Err("serve public base URL is required for a serve node");
        };
        let Ok(base_url) = url::Url::parse(base_url) else {
            return Err("serve public base URL must be an absolute HTTP(S) URL");
        };
        if !matches!(base_url.scheme(), "http" | "https") || !base_url.has_host() {
            return Err("serve public base URL must be an absolute HTTP(S) URL");
        }
        let Some(resources) = body.resources else {
            return Err("serve resources are required for a serve node");
        };
        if resources.cpu_millicores == 0
            || resources.memory_mb == 0
            || resources.disk_mb == 0
            || resources.max_deployments == 0
            || i64::try_from(resources.cpu_millicores).is_err()
            || i64::try_from(resources.memory_mb).is_err()
            || i64::try_from(resources.disk_mb).is_err()
            || i32::try_from(resources.max_deployments).is_err()
        {
            return Err("serve resources must contain positive supported values");
        }
    } else if body.serve_base_url.is_some() || body.resources.is_some() {
        return Err("serve settings are not allowed for a non-serve node");
    }
    Ok(())
}

fn validate_existing_policy(
    body: &RegisterRequest,
    persisted: GatewayAuthenticationMode,
) -> Result<(), &'static str> {
    if body.effective_config.is_none() && body.gateway_authentication != persisted {
        return Err("effective configuration is required when changing gateway authentication");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(build: bool, serve: bool) -> RegisterRequest {
        RegisterRequest {
            name: "node-a".to_owned(),
            version: "0.1.0".to_owned(),
            capabilities: NodeCapabilities { build, serve },
            region: "default".to_owned(),
            build_concurrency: u16::from(build),
            serve_base_url: serve.then(|| "http://node-a:8080".to_owned()),
            resources: serve.then_some(grass_node_protocol::NodeResources {
                cpu_millicores: 800,
                memory_mb: 768,
                disk_mb: 4_096,
                max_deployments: 10,
            }),
            config_revision: 0,
            effective_config: None,
            node_token_configured: false,
            gateway_authentication: GatewayAuthenticationMode::Token,
        }
    }

    #[test]
    fn registration_requires_at_least_one_capability() {
        assert_eq!(
            validate_registration(&request(false, false)).unwrap_err(),
            "node must enable build or serve"
        );
        assert!(validate_registration(&request(true, false)).is_ok());
        assert!(validate_registration(&request(false, true)).is_ok());
    }

    #[test]
    fn no_token_registration_requires_persistable_effective_policy() {
        let mut body = request(false, true);
        body.gateway_authentication = GatewayAuthenticationMode::None;
        assert_eq!(
            validate_registration(&body),
            Err("effective configuration is required for no-token gateways")
        );
    }

    #[test]
    fn legacy_registration_cannot_reuse_a_stale_no_token_policy() {
        let body = request(false, true);
        assert!(validate_registration(&body).is_ok());
        assert!(validate_existing_policy(&body, GatewayAuthenticationMode::Token).is_ok());
        assert_eq!(
            validate_existing_policy(&body, GatewayAuthenticationMode::None),
            Err("effective configuration is required when changing gateway authentication")
        );
    }

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<
            RegisterRequest,
            grass_node_protocol::RegisterRequest,
        >("RegisterRequest");
        crate::test_support::assert_node_contract::<
            RegisterResponse,
            grass_node_protocol::RegisterResponse,
        >("RegisterResponse");
    }
}
