use axum::{
    Extension, Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::{deployments, ssh_host_keys},
    infra::{
        database::entity::{deployment, node},
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

#[derive(Clone, Serialize, Deserialize)]
struct ObserveSshHostKeyRequest {
    host: String,
    port: u16,
    key_type: String,
    public_key: String,
    fingerprint_sha256: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct ObserveSshHostKeyResponse {
    approved: bool,
    #[serde(default)]
    known_hosts_line: Option<String>,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route(
        "/deployments/{deployment_id}/ssh-host-key",
        axum::routing::post(observe_ssh_host_key),
    )
}

async fn build_owned_deployment(
    db: &sea_orm::DatabaseConnection,
    node: &node::Model,
    deployment_id: Uuid,
    op: &'static str,
) -> Result<deployment::Model, AppError> {
    let deployment = deployments::get_by_id(db, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "deployment not found".to_owned(),
        })?;
    if deployment.build_node_id != Some(node.id) {
        return Err(AppError::Forbidden {
            op,
            message: "deployment build is not assigned to this node".to_owned(),
        });
    }
    Ok(deployment)
}

/// POST /api/v1/internal/deployments/{deployment_id}/ssh-host-key
async fn observe_ssh_host_key(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
    Json(body): Json<ObserveSshHostKeyRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.deployments.ssh_host_key";
    let db = crate::infra::http::database(&state, OP)?;
    let deployment = build_owned_deployment(db, &node, deployment_id, OP).await?;
    let endpoint = deployment
        .source_repository_url
        .as_deref()
        .and_then(|url| grass_git_source::parse_repository_url(url).ok())
        .filter(|endpoint| endpoint.transport == grass_git_source::GitTransport::Ssh)
        .ok_or_else(|| AppError::Validation {
            op: OP,
            message: "deployment does not use an SSH repository".to_owned(),
        })?;
    if !endpoint.host.eq_ignore_ascii_case(&body.host) || endpoint.port != body.port {
        return Err(AppError::Validation {
            op: OP,
            message: "SSH host key endpoint does not match deployment".to_owned(),
        });
    }
    let key = ssh_host_keys::observe(
        db,
        ssh_host_keys::ObserveHostKeyParams {
            team_id: deployment.team_id,
            host: endpoint.host,
            port: endpoint.port,
            key_type: body.key_type,
            public_key: body.public_key,
            fingerprint_sha256: body.fingerprint_sha256,
            node_id: node.id,
        },
    )
    .await
    .map_err(|error| match error {
        ssh_host_keys::SshHostKeyError::Invalid => AppError::Validation {
            op: OP,
            message: "SSH host key payload is invalid".to_owned(),
        },
        ssh_host_keys::SshHostKeyError::NotFound => AppError::NotFound {
            op: OP,
            message: "SSH host key not found".to_owned(),
        },
        ssh_host_keys::SshHostKeyError::Database(source) => AppError::Infrastructure {
            op: OP,
            source: source.into(),
        },
    })?;
    let approved = key.status == crate::infra::database::entity::SshHostKeyStatus::Approved;
    Ok(ok_response(ObserveSshHostKeyResponse {
        approved,
        known_hosts_line: approved.then(|| ssh_host_keys::known_hosts_line(&key)),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<
            ObserveSshHostKeyRequest,
            grass_node_protocol::ObserveSshHostKeyRequest,
        >("ObserveSshHostKeyRequest");
        crate::test_support::assert_node_contract::<
            ObserveSshHostKeyResponse,
            grass_node_protocol::ObserveSshHostKeyResponse,
        >("ObserveSshHostKeyResponse");
    }
}
