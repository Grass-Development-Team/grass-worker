use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::nodes::{self, HEARTBEAT_STALE_SECONDS},
    infra::error::{AppError, ok_response},
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/nodes/{node_id}/health", axum::routing::get(health))
}

/// GET /api/v1/admin/nodes/{node_id}/health
async fn health(
    State(state): State<ControlApiState>,
    Path(node_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.health";
    let db = crate::infra::http::database(&state, OP)?;

    let node = nodes::get_by_id(db, node_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "node not found".to_owned(),
        })?;

    let now = OffsetDateTime::now_utc();
    Ok(ok_response(HealthResponse {
        node_id: node.id,
        status: nodes::status_value(&node.status),
        healthy: nodes::is_healthy(&node, now, HEARTBEAT_STALE_SECONDS),
        last_heartbeat_at: node.last_heartbeat_at,
        seconds_since_heartbeat: node.last_heartbeat_at.map(|at| (now - at).whole_seconds()),
    }))
}

#[derive(serde::Serialize)]
struct HealthResponse {
    node_id: uuid::Uuid,
    status: &'static str,
    healthy: bool,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    last_heartbeat_at: Option<time::OffsetDateTime>,
    seconds_since_heartbeat: Option<i64>,
}
