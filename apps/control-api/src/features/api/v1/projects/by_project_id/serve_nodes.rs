use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Deserialize;
use std::collections::HashMap;
use uuid::Uuid;

use crate::{
    domain::{deployments, scheduler},
    infra::{
        database::entity::node,
        error::{AppError, ok_response},
        http::{deployment_errors::map_schedule_error, extractors::Session},
    },
    state::ControlApiState,
};

#[derive(serde::Serialize)]
struct ServeNodeResponse {
    id: Uuid,
    name: String,
    region: String,
    healthy: bool,
    capacity: grass_node_protocol::NodeResources,
    usage: scheduler::NodeUsage,
    normal_available: bool,
    schedulable: bool,
    overflow_only: bool,
    disk_available_mb: u64,
    remaining_overflow_slots: u64,
}

#[derive(serde::Serialize)]
struct ServeNodesResponse {
    serve_nodes: Vec<ServeNodeResponse>,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/serve-nodes",
        axum::routing::get(serve_nodes),
    )
}

#[derive(Debug, Default, Deserialize)]
struct ServeNodesQuery {
    #[serde(default)]
    region: Option<String>,
}

/// GET /api/v1/projects/{project_id}/serve-nodes
async fn serve_nodes(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
    Query(query): Query<ServeNodesQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.serve_nodes";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    let db = crate::infra::http::database(&state, OP)?;
    let requested = deployments::runtime_serve_resources(&access.project.runtime);
    let requested_region = query
        .region
        .as_deref()
        .map(grass_validator::normalize_region)
        .transpose()
        .map_err(|error| AppError::Validation {
            op: OP,
            message: format!("region: {error}"),
        })?;
    let candidates = scheduler::eligible_candidates(db)
        .await
        .map_err(|error| map_schedule_error(error, OP))?;
    let node_ids: Vec<Uuid> = candidates
        .iter()
        .map(|candidate| candidate.node_id)
        .collect();
    let nodes: HashMap<Uuid, node::Model> = if node_ids.is_empty() {
        HashMap::new()
    } else {
        node::Entity::find()
            .filter(node::Column::Id.is_in(node_ids))
            .all(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .into_iter()
            .map(|node| (node.id, node))
            .collect()
    };

    let views = candidates
        .iter()
        .filter_map(|candidate| {
            if requested_region
                .as_deref()
                .is_some_and(|region| candidate.region != region)
            {
                return None;
            }
            let node = nodes.get(&candidate.node_id)?;
            let placement =
                scheduler::choose_candidate(std::slice::from_ref(candidate), requested, None).ok();
            let max_deployments = u64::from(candidate.capacity.max_deployments);
            let overflow_used = candidate.usage.deployments.saturating_sub(max_deployments);
            Some(ServeNodeResponse {
                id: node.id,
                name: node.name.clone(),
                region: candidate.region.clone(),
                healthy: true,
                capacity: candidate.capacity,
                usage: candidate.usage,
                normal_available: placement
                    .as_ref()
                    .is_some_and(|placement| !placement.overcommitted),
                schedulable: placement.is_some(),
                overflow_only: placement.is_some_and(|placement| placement.overcommitted),
                disk_available_mb: candidate
                    .capacity
                    .disk_mb
                    .saturating_sub(candidate.usage.disk_mb),
                remaining_overflow_slots: 2_u64.saturating_sub(overflow_used),
            })
        })
        .collect::<Vec<_>>();

    Ok(ok_response(ServeNodesResponse { serve_nodes: views }))
}
