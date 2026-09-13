use axum::{
    Extension, Json,
    extract::{Path, State},
    response::IntoResponse,
};
use grass_node_protocol::GitCredential;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::{deployments, source_credentials},
    infra::{
        database::entity::{deployment, node},
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct RedeemGitCredentialRequest {
    pub lease: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct RedeemGitCredentialResponse {
    pub credential: GitCredential,
    pub host: String,
    pub port: u16,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route(
        "/deployments/{deployment_id}/source-credential",
        axum::routing::post(redeem_source_credential),
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

/// POST /api/v1/internal/deployments/{deployment_id}/source-credential
pub async fn redeem_source_credential(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
    Json(body): Json<RedeemGitCredentialRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.deployments.source_credential";
    let db = crate::infra::http::database(&state, OP)?;
    build_owned_deployment(db, &node, deployment_id, OP).await?;
    let keyring = state.config.read().unwrap().secrets.git_credentials.clone();
    let redeemed =
        source_credentials::redeem_lease(db, &keyring, node.id, deployment_id, &body.lease)
            .await
            .map_err(|error| match error {
                source_credentials::SourceCredentialError::InvalidLease => AppError::Unauthorized {
                    op: OP,
                    message: "source credential lease is invalid or expired".to_owned(),
                },
                source_credentials::SourceCredentialError::Revoked => AppError::Conflict {
                    op: OP,
                    message: "source credential has been revoked".to_owned(),
                },
                source_credentials::SourceCredentialError::Database(source) => {
                    AppError::Infrastructure {
                        op: OP,
                        source: source.into(),
                    }
                }
                source_credentials::SourceCredentialError::Other(source) => {
                    AppError::Infrastructure { op: OP, source }
                }
                _ => AppError::Internal {
                    op: OP,
                    message: "source credential could not be decrypted".to_owned(),
                },
            })?;
    Ok(ok_response(RedeemGitCredentialResponse {
        credential: redeemed.credential,
        host: redeemed.host,
        port: redeemed.port,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<
            RedeemGitCredentialRequest,
            grass_node_protocol::RedeemGitCredentialRequest,
        >("RedeemGitCredentialRequest");
        crate::test_support::assert_node_contract::<
            RedeemGitCredentialResponse,
            grass_node_protocol::RedeemGitCredentialResponse,
        >("RedeemGitCredentialResponse");
    }
}
