use axum::{Extension, extract::State, response::IntoResponse};
use grass_node_protocol::{CertificateBundle, HttpChallenge};
use serde::{Deserialize, Serialize};

use crate::{
    infra::{
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CertificateBundlesResponse {
    pub bundles: Vec<CertificateBundle>,
    #[serde(default)]
    pub challenges: Vec<HttpChallenge>,
    #[serde(default)]
    pub challenge_revision: String,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route("/serve/certificates", axum::routing::get(certificates))
}

fn ensure_serve_node(
    node: &crate::infra::database::entity::node::Model,
    op: &'static str,
) -> Result<(), AppError> {
    if !node.serve_enabled {
        return Err(AppError::Forbidden {
            op,
            message: "node does not have Serve capability".to_owned(),
        });
    }
    Ok(())
}

/// GET /api/v1/internal/serve/certificates
pub async fn certificates(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.serve.certificates";
    ensure_serve_node(&node, OP)?;
    let db = crate::infra::http::database(&state, OP)?;
    let platform_secret = state.config.read().unwrap().secrets.secret_key.clone();
    let snapshot = crate::domain::certificates::snapshot(db, &node.region, &platform_secret)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(CertificateBundlesResponse {
        bundles: snapshot.bundles,
        challenges: snapshot.challenges,
        challenge_revision: snapshot.challenge_revision,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<
            CertificateBundlesResponse,
            grass_node_protocol::CertificateBundlesResponse,
        >("CertificateBundlesResponse");
    }
}
