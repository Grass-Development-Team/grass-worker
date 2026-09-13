use axum::{
    Extension, Json,
    extract::{Path, State},
    response::IntoResponse,
};
use grass_cache::Cache;
use grass_node_protocol::BuildLogLine;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::deployments,
    infra::{
        database::entity::{deployment, node},
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppendBuildLogRequest {
    lines: Vec<BuildLogLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppendBuildLogResponse {
    last_seq: u64,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route(
        "/deployments/{deployment_id}/build-log",
        axum::routing::put(append_build_log),
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

// --- Build log --------------------------------------------------------------
fn log_seq_key(deployment_id: Uuid) -> String {
    format!("deployment:{deployment_id}:log_seq")
}

/// PUT /api/v1/internal/deployments/{deployment_id}/build-log
async fn append_build_log(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
    Json(body): Json<AppendBuildLogRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.deployments.build_log";
    let db = crate::infra::http::database(&state, OP)?;
    let cache = crate::infra::http::cache(&state, OP)?;
    let deployment = build_owned_deployment(db, &node, deployment_id, OP).await?;

    if body.lines.is_empty() {
        return Ok(ok_response(AppendBuildLogResponse { last_seq: 0 }));
    }

    // Stored as JSON lines so the catch-up API can filter by sequence.
    let mut buffer = String::new();
    let mut last_seq = 0;
    for line in &body.lines {
        buffer.push_str(&serde_json::to_string(line).map_err(|source| {
            AppError::Infrastructure {
                op: OP,
                source: source.into(),
            }
        })?);
        buffer.push('\n');
        last_seq = last_seq.max(line.seq);
    }

    state
        .storage
        .clone()
        .append_build_log(deployment.project_id, deployment.id, &buffer)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    let _ = cache
        .set(
            &log_seq_key(deployment.id),
            &last_seq.to_string(),
            std::time::Duration::from_secs(60 * 60 * 24),
        )
        .await;

    Ok(ok_response(AppendBuildLogResponse { last_seq }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<
            AppendBuildLogRequest,
            grass_node_protocol::AppendBuildLogRequest,
        >("AppendBuildLogRequest");
        crate::test_support::assert_node_contract::<
            AppendBuildLogResponse,
            grass_node_protocol::AppendBuildLogResponse,
        >("AppendBuildLogResponse");
    }
}
