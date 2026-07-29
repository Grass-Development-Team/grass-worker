use axum::{Extension, Json, extract::State, response::IntoResponse};
use grass_node_protocol::{
    HeartbeatRequest, HeartbeatResponse, NodeCapabilities, RegisterRequest, RegisterResponse,
};
use ring::hmac;
use serde_json::json;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        nodes::{self, RegisterNodeParams},
    },
    infra::{
        database::entity::AuditEventResult,
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

/// POST /api/v1/internal/nodes/register
pub async fn register(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Json(body): Json<RegisterRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.nodes.register";
    let db = super::database(&state, OP)?;

    validate_registration(&body).map_err(|message| AppError::Validation {
        op: OP,
        message: message.to_owned(),
    })?;
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
            version: body.version,
            build_enabled: body.capabilities.build,
            serve_enabled: body.capabilities.serve,
            build_concurrency: i32::from(body.build_concurrency),
            base_url: body.serve_base_url,
            resources: body.resources,
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    let _ = audits::create_audit_event(
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
    .await;

    let gateway_token = serve_enabled
        .then(|| derive_gateway_token(&state.config.read().unwrap().secrets.secret_key));
    Ok(ok_response(RegisterResponse {
        node_id: node.id,
        name: node.name,
        capabilities: NodeCapabilities {
            build: build_enabled,
            serve: serve_enabled,
        },
        gateway_token,
    }))
}

fn validate_registration(body: &RegisterRequest) -> Result<(), &'static str> {
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

fn derive_gateway_token(secret: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    hex::encode(hmac::sign(&key, b"grass-node-gateway-v1").as_ref())
}

/// POST /api/v1/internal/nodes/heartbeat
pub async fn heartbeat(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Json(_body): Json<HeartbeatRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.nodes.heartbeat";
    let db = super::database(&state, OP)?;

    nodes::record_heartbeat(db, node)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(HeartbeatResponse { acknowledged: true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(build: bool, serve: bool) -> RegisterRequest {
        RegisterRequest {
            name: "node-a".to_owned(),
            version: "0.1.0".to_owned(),
            capabilities: NodeCapabilities { build, serve },
            build_concurrency: u16::from(build),
            serve_base_url: serve.then(|| "http://node-a:8080".to_owned()),
            resources: serve.then_some(grass_node_protocol::NodeResources {
                cpu_millicores: 800,
                memory_mb: 768,
                disk_mb: 4_096,
                max_deployments: 10,
            }),
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
    fn gateway_token_is_stable_and_secret_derived() {
        let first = derive_gateway_token("a sufficiently long control api secret");
        let second = derive_gateway_token("a sufficiently long control api secret");
        let different = derive_gateway_token("another sufficiently long api secret");

        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert_ne!(first, different);
    }
}
