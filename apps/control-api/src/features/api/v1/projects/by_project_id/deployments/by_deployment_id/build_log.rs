use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
};
use grass_node_protocol::BuildLogLine;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::deployments,
    infra::{
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

#[derive(serde::Serialize)]
struct BuildLogResponse {
    lines: Vec<BuildLogLine>,
    last_seq: u64,
    build_status: &'static str,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/deployments/{deployment_id}/build-log",
        axum::routing::get(build_log),
    )
}

#[derive(Deserialize)]
struct BuildLogQuery {
    #[serde(default)]
    after_seq: Option<u64>,
}

/// GET /api/v1/projects/{project_id}/deployments/{deployment_id}/build-log
///
/// Returns persisted log lines with `seq > after_seq` so reconnecting
/// clients can catch up before resuming the websocket stream.
async fn build_log(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<BuildLogQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.build_log";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    let db = crate::infra::http::database(&state, OP)?;
    let deployment = deployments::get_by_id(db, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .filter(|deployment| deployment.project_id == access.project.id)
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment not found".to_owned(),
        })?;

    let storage = state.storage.clone();
    let after_seq = query.after_seq.unwrap_or(0);
    let content = storage
        .read_build_log(deployment.project_id, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .unwrap_or_default();

    let mut lines: Vec<BuildLogLine> = Vec::new();
    let mut last_seq = 0;
    for raw in content.lines() {
        let Ok(line) = serde_json::from_str::<BuildLogLine>(raw) else {
            continue;
        };
        last_seq = last_seq.max(line.seq);
        if line.seq > after_seq {
            lines.push(line);
        }
    }

    Ok(ok_response(BuildLogResponse {
        lines,
        last_seq,
        build_status: deployments::build_status_value(&deployment.build_status),
    }))
}
