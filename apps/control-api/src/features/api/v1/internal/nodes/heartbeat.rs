use axum::{Extension, Json, extract::State, response::IntoResponse};
use grass_node_protocol::NodeConfiguration;
use serde::{Deserialize, Serialize};

use crate::{
    domain::nodes,
    infra::{
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct HeartbeatRequest {
    /// Number of builds currently running on the Node.
    #[serde(default)]
    pub active_builds: u16,
    /// Revision currently used by the running process.
    #[serde(default)]
    pub effective_config_revision: u64,
    /// Desired revision written to disk and awaiting process restart.
    #[serde(default)]
    pub applying_config_revision: Option<u64>,
    /// Last failure while persisting the desired configuration.
    #[serde(default)]
    pub config_apply_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct HeartbeatResponse {
    pub acknowledged: bool,
    /// Latest desired revision, when it differs from the running process.
    #[serde(default)]
    pub desired_config_revision: Option<u64>,
    /// Complete desired non-secret configuration for the Node to persist.
    #[serde(default)]
    pub desired_config: Option<NodeConfiguration>,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route("/nodes/heartbeat", axum::routing::post(heartbeat))
}

/// POST /api/v1/internal/nodes/heartbeat
pub async fn heartbeat(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Json(body): Json<HeartbeatRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.nodes.heartbeat";
    let db = crate::infra::http::database(&state, OP)?;

    let report = grass_node_protocol::HeartbeatRequest {
        active_builds: body.active_builds,
        effective_config_revision: body.effective_config_revision,
        applying_config_revision: body.applying_config_revision,
        config_apply_error: body.config_apply_error,
    };
    let node = nodes::record_heartbeat(db, node, &report)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let (desired_config_revision, desired_config) =
        nodes::desired_config_for_heartbeat(&node, &report)
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(HeartbeatResponse {
        acknowledged: true,
        desired_config_revision,
        desired_config,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_token_is_stable_and_secret_derived() {
        let first = nodes::gateway_token("a sufficiently long control api secret");
        let second = nodes::gateway_token("a sufficiently long control api secret");
        let different = nodes::gateway_token("another sufficiently long api secret");

        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert_ne!(first, different);
    }

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<
            HeartbeatRequest,
            grass_node_protocol::HeartbeatRequest,
        >("HeartbeatRequest");
        crate::test_support::assert_node_contract::<
            HeartbeatResponse,
            grass_node_protocol::HeartbeatResponse,
        >("HeartbeatResponse");
    }
}
