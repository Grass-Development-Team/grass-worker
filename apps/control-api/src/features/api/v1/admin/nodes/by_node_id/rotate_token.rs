use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::nodes,
    infra::{
        audit::CreateAuditEventParams,
        database::entity::AuditEventResult,
        error::{AppError, ok_response},
        http::middlewares::node_auth::revoked_token_key,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/nodes/{node_id}/rotate-token",
        axum::routing::post(rotate_token),
    )
}

/// POST /api/v1/admin/nodes/{node_id}/rotate-token
///
/// Revokes the current token immediately and returns a new one once.
async fn rotate_token(
    State(state): State<ControlApiState>,
    crate::infra::http::extractors::Session { data, .. }: crate::infra::http::extractors::Session,
    Path(node_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.rotate_token";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;

    let node = nodes::get_by_id(db, node_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "node not found".to_owned(),
        })?;

    let old_hash = node.token_hash.clone();
    let token = grass_token::generate_token();
    let node = nodes::replace_token_hash(db, node, grass_token::hash_token(&token))
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "node.token_revoked".to_owned(),
            target_type: "node".to_owned(),
            target_id: Some(node.id),
            result: AuditEventResult::Success,
            reason: Some("token rotated".to_owned()),
            metadata: json!({}),
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
    // Blacklist the old token so revocation applies before any cache of the
    // node row expires.
    if let Some(cache) = state.try_cache() {
        use grass_cache::Cache;
        let _ = cache
            .set(
                &revoked_token_key(&old_hash),
                "1",
                std::time::Duration::from_secs(60 * 60 * 24 * 30),
            )
            .await;
    }

    Ok(ok_response(RotateTokenResponse {
        node_id: node.id,
        token,
    }))
}

#[derive(serde::Serialize)]
struct RotateTokenResponse {
    node_id: uuid::Uuid,
    token: String,
}
