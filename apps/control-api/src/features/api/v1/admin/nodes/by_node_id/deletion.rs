use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{node_deletions, nodes, scheduler},
    infra::{
        database::entity::node_deletion_job,
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/nodes/{node_id}/deletion",
        axum::routing::post(queue_deletion),
    )
}

#[derive(Deserialize)]
pub struct QueueNodeDeletionRequest {
    pub target_node_id: Option<Uuid>,
}

fn deletion_job_view(job: &node_deletion_job::Model) -> NodeDeletionResponse {
    NodeDeletionResponse {
        id: job.id,
        status: node_deletions::status_value(&job.status),
        target_node_id: job.target_node_id,
        total_deployments: job.total_deployments,
        migrated_deployments: job.migrated_deployments,
        active_builds: job.active_builds,
        error: job.error.clone(),
        created_at: job.created_at,
        updated_at: job.updated_at,
        completed_at: job.completed_at,
    }
}

/// POST /api/v1/admin/nodes/{node_id}/deletion
pub async fn queue_deletion(
    State(state): State<ControlApiState>,
    crate::infra::http::extractors::Session { data, .. }: crate::infra::http::extractors::Session,
    Path(node_id): Path<Uuid>,
    Json(body): Json<QueueNodeDeletionRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.queue_deletion";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    scheduler::lock_placement(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let node = nodes::get_by_id_for_update(&transaction, node_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "node not found".to_owned(),
        })?;
    let job = node_deletions::enqueue(&transaction, node, body.target_node_id, data.user_id)
        .await
        .map_err(|error| AppError::Conflict {
            op: OP,
            message: error.to_string(),
        })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(QueueDeletionResponse {
        job: deletion_job_view(&job),
    }))
}

#[derive(serde::Serialize)]
struct NodeDeletionResponse {
    id: uuid::Uuid,
    status: &'static str,
    target_node_id: Option<uuid::Uuid>,
    total_deployments: i32,
    migrated_deployments: i32,
    active_builds: i32,
    error: Option<String>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    updated_at: time::OffsetDateTime,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    completed_at: Option<time::OffsetDateTime>,
}

#[derive(serde::Serialize)]
struct QueueDeletionResponse {
    job: NodeDeletionResponse,
}
