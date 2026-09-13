use axum::{
    Extension, Json,
    extract::State,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::{
    domain::preview_access::{
        AUTHORIZATION_STATE_TTL, AuthorizationStateRecord, configured_site_url, expires_after,
        resolve_preview_binding, site_route_url, store_record, validate_return_to,
    },
    infra::{
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

#[cfg_attr(test, derive(serde::Deserialize))]
#[derive(Serialize)]
struct StartPreviewAuthorizationResponse {
    authorization_url: String,
}

#[cfg_attr(test, derive(serde::Serialize))]
#[derive(Deserialize)]
struct StartPreviewAuthorizationRequest {
    host: String,
    return_to: String,
}

/// POST /api/v1/internal/serve/preview/authorize
async fn start(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(_node)): Extension<AuthenticatedNode>,
    Json(body): Json<StartPreviewAuthorizationRequest>,
) -> Result<Response, AppError> {
    const OP: &str = "internal.serve.preview_authorize";
    let db = crate::infra::http::database(&state, OP)?;
    let cache = crate::infra::http::cache(&state, OP)?;
    let binding = resolve_preview_binding(db, &body.host, OP).await?;
    let return_to = validate_return_to(&body.return_to).map_err(|error| AppError::Validation {
        op: OP,
        message: error.to_string(),
    })?;
    let token = grass_token::generate_token();
    store_record(
        cache,
        "authorization",
        &token,
        &AuthorizationStateRecord {
            binding,
            return_to,
            expires_at: expires_after(AUTHORIZATION_STATE_TTL),
        },
        AUTHORIZATION_STATE_TTL,
        OP,
    )
    .await?;
    let site_url = configured_site_url(db, OP).await?;
    let authorization_url = site_route_url(&site_url, "/api/v1/preview/authorize", "state", &token)
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(StartPreviewAuthorizationResponse { authorization_url }).into_response())
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route("/serve/preview/authorize", axum::routing::post(start))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<
            StartPreviewAuthorizationResponse,
            grass_node_protocol::StartPreviewAuthorizationResponse,
        >("StartPreviewAuthorizationResponse");
        crate::test_support::assert_node_contract::<
            StartPreviewAuthorizationRequest,
            grass_node_protocol::StartPreviewAuthorizationRequest,
        >("StartPreviewAuthorizationRequest");
    }
}
