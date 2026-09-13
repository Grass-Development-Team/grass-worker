use axum::{
    Extension, Json,
    extract::State,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::{
    domain::preview_access::{
        PreviewGrantRecord, ScreenshotGrantRecord, get_record, is_expired, require_current_binding,
        user_can_access_preview, validate_source_session,
    },
    infra::{
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

#[cfg_attr(test, derive(serde::Deserialize))]
#[derive(Serialize)]
struct VerifyPreviewGrantResponse {
    allowed: bool,
}

#[cfg_attr(test, derive(serde::Serialize))]
#[derive(Deserialize)]
struct VerifyPreviewGrantRequest {
    host: String,
    grant: String,
}

/// POST /api/v1/internal/serve/preview/verify
async fn verify(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(_node)): Extension<AuthenticatedNode>,
    Json(body): Json<VerifyPreviewGrantRequest>,
) -> Result<Response, AppError> {
    const OP: &str = "internal.serve.preview_verify";
    let db = crate::infra::http::database(&state, OP)?;
    let cache = crate::infra::http::cache(&state, OP)?;
    let host =
        grass_validator::normalize_host(&body.host).map_err(|error| AppError::Validation {
            op: OP,
            message: error.to_string(),
        })?;
    if let Some(record) =
        get_record::<ScreenshotGrantRecord>(cache, "screenshot-grant", &body.grant, OP)
            .await?
            .filter(|record| !is_expired(record.expires_at) && record.binding.host == host)
    {
        require_current_binding(db, &record.binding, OP).await?;
        return Ok(ok_response(VerifyPreviewGrantResponse { allowed: true }).into_response());
    }
    let record = get_record::<PreviewGrantRecord>(cache, "grant", &body.grant, OP)
        .await?
        .filter(|record| !is_expired(record.expires_at) && record.binding.host == host)
        .ok_or_else(|| AppError::Unauthorized {
            op: OP,
            message: "preview grant is invalid or expired".to_owned(),
        })?;
    require_current_binding(db, &record.binding, OP).await?;
    let session = validate_source_session(&state, &record.session_id, OP)
        .await?
        .filter(|session| session.user_id == record.user_id)
        .ok_or_else(|| AppError::Unauthorized {
            op: OP,
            message: "preview session is invalid or expired".to_owned(),
        })?;
    if session.user_id != record.user_id {
        return Err(AppError::Unauthorized {
            op: OP,
            message: "preview session is invalid or expired".to_owned(),
        });
    }
    if !user_can_access_preview(db, record.binding.team_id, record.user_id, OP).await? {
        return Err(AppError::Forbidden {
            op: OP,
            message: "preview access requires team membership or platform administration"
                .to_owned(),
        });
    }
    Ok(ok_response(VerifyPreviewGrantResponse { allowed: true }).into_response())
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route("/serve/preview/verify", axum::routing::post(verify))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<
            VerifyPreviewGrantResponse,
            grass_node_protocol::VerifyPreviewGrantResponse,
        >("VerifyPreviewGrantResponse");
        crate::test_support::assert_node_contract::<
            VerifyPreviewGrantRequest,
            grass_node_protocol::VerifyPreviewGrantRequest,
        >("VerifyPreviewGrantRequest");
    }
}
