use axum::{
    Extension, Json,
    extract::State,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use time::OffsetDateTime;

use crate::{
    domain::preview_access::{
        CallbackCodeRecord, PREVIEW_GRANT_TTL, PreviewGrantRecord, is_expired,
        require_current_binding, store_record, take_record, user_can_access_preview,
        validate_source_session,
    },
    infra::{
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

#[cfg_attr(test, derive(serde::Deserialize))]
#[derive(Serialize)]
pub struct ExchangePreviewCodeResponse {
    pub grant: String,
    pub return_to: String,
    pub max_age_seconds: u64,
    pub cookie_secure: bool,
}

#[cfg_attr(test, derive(serde::Serialize))]
#[derive(Deserialize)]
pub struct ExchangePreviewCodeRequest {
    pub host: String,
    pub code: String,
}

/// POST /api/v1/internal/serve/preview/exchange
pub async fn exchange(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(_node)): Extension<AuthenticatedNode>,
    Json(body): Json<ExchangePreviewCodeRequest>,
) -> Result<Response, AppError> {
    const OP: &str = "internal.serve.preview_exchange";
    let db = crate::infra::http::database(&state, OP)?;
    let cache = crate::infra::http::cache(&state, OP)?;
    let host =
        grass_validator::normalize_host(&body.host).map_err(|error| AppError::Validation {
            op: OP,
            message: error.to_string(),
        })?;
    let record = take_record::<CallbackCodeRecord>(cache, "callback", &body.code, OP)
        .await?
        .filter(|record| !is_expired(record.expires_at) && record.binding.host == host)
        .ok_or_else(|| AppError::Unauthorized {
            op: OP,
            message: "preview callback code is invalid or expired".to_owned(),
        })?;
    require_current_binding(db, &record.binding, OP).await?;
    let session = validate_source_session(&state, &record.session_id, OP)
        .await?
        .filter(|session| session.user_id == record.user_id)
        .ok_or_else(|| AppError::Unauthorized {
            op: OP,
            message: "preview session is invalid or expired".to_owned(),
        })?;
    if !user_can_access_preview(db, record.binding.team_id, record.user_id, OP).await? {
        return Err(AppError::Forbidden {
            op: OP,
            message: "preview access requires team membership or platform administration"
                .to_owned(),
        });
    }

    let now = OffsetDateTime::now_utc().unix_timestamp();
    let absolute_session_expiry = session.created_at.unix_timestamp()
        + state.config.read().unwrap().session.session_ttl_seconds as i64;
    let max_age_seconds = PREVIEW_GRANT_TTL
        .as_secs()
        .min((absolute_session_expiry - now).max(0) as u64);
    if max_age_seconds == 0 {
        return Err(AppError::Unauthorized {
            op: OP,
            message: "preview session is invalid or expired".to_owned(),
        });
    }

    let grant = grass_token::generate_token();
    store_record(
        cache,
        "grant",
        &grant,
        &PreviewGrantRecord {
            binding: record.binding,
            session_id: record.session_id,
            user_id: record.user_id,
            expires_at: now + max_age_seconds as i64,
        },
        Duration::from_secs(max_age_seconds),
        OP,
    )
    .await?;
    Ok(ok_response(ExchangePreviewCodeResponse {
        grant,
        return_to: record.return_to,
        max_age_seconds,
        cookie_secure: record.cookie_secure,
    })
    .into_response())
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route("/serve/preview/exchange", axum::routing::post(exchange))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<
            ExchangePreviewCodeResponse,
            grass_node_protocol::ExchangePreviewCodeResponse,
        >("ExchangePreviewCodeResponse");
        crate::test_support::assert_node_contract::<
            ExchangePreviewCodeRequest,
            grass_node_protocol::ExchangePreviewCodeRequest,
        >("ExchangePreviewCodeRequest");
    }
}
