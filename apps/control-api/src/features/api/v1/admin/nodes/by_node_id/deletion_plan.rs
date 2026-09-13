use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use uuid::Uuid;

use crate::{
    domain::{node_deletions, nodes},
    infra::error::{AppError, ok_response},
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/nodes/{node_id}/deletion-plan",
        axum::routing::get(deletion_plan),
    )
}

/// GET /api/v1/admin/nodes/{node_id}/deletion-plan
pub async fn deletion_plan(
    State(state): State<ControlApiState>,
    Path(node_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.deletion_plan";
    let db = crate::infra::http::database(&state, OP)?;
    let node = nodes::get_by_id(db, node_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "node not found".to_owned(),
        })?;
    let plan = node_deletions::plan(db, &node)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(DeletionPlanResponse {
        node_id: plan.node_id,
        assigned_deployments: plan.assigned_deployments,
        active_builds: plan.active_builds,
        requires_target: plan.requires_target,
        eligible_targets: plan
            .eligible_targets
            .into_iter()
            .map(|target| EligibleTargetResponse {
                id: target.id,
                name: target.name,
                available_deployments: target.available_deployments,
            })
            .collect(),
    }))
}

#[derive(serde::Serialize)]
struct DeletionPlanResponse {
    node_id: Uuid,
    assigned_deployments: u64,
    active_builds: u64,
    requires_target: bool,
    eligible_targets: Vec<EligibleTargetResponse>,
}
#[derive(serde::Serialize)]
struct EligibleTargetResponse {
    id: Uuid,
    name: String,
    available_deployments: u64,
}
