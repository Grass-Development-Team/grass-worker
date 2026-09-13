use axum::{Json, extract::State, response::IntoResponse};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, future::Future};
use uuid::Uuid;

use crate::{
    infra::{
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/projects/batch", axum::routing::post(batch))
}

#[derive(Debug, Serialize)]
pub struct BatchItemResult {
    pub id: Uuid,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub fn normalize_ids(ids: Vec<Uuid>, op: &'static str) -> Result<Vec<Uuid>, AppError> {
    if ids.is_empty() || ids.len() > 100 {
        return Err(AppError::Validation {
            op,
            message: "ids must contain between 1 and 100 items".to_owned(),
        });
    }

    let mut seen = HashSet::with_capacity(ids.len());
    Ok(ids.into_iter().filter(|id| seen.insert(*id)).collect())
}

pub async fn run<F, Fut>(ids: Vec<Uuid>, mut operation: F) -> Vec<BatchItemResult>
where
    F: FnMut(Uuid) -> Fut,
    Fut: Future<Output = Result<(), AppError>>,
{
    let mut results = Vec::with_capacity(ids.len());
    for id in ids {
        match operation(id).await {
            Ok(()) => results.push(BatchItemResult {
                id,
                success: true,
                code: None,
                message: None,
            }),
            Err(error) => results.push(BatchItemResult {
                id,
                success: false,
                code: Some(error.error_code()),
                message: Some(error.to_string()),
            }),
        }
    }
    results
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ProjectBatchRequest {
    Archive { ids: Vec<Uuid> },
    Unarchive { ids: Vec<Uuid> },
    Delete { ids: Vec<Uuid> },
}

#[derive(Clone, Copy)]
enum ProjectBatchAction {
    Archive,
    Unarchive,
    Delete,
}

/// POST /api/v1/admin/projects/batch
pub async fn batch(
    State(state): State<ControlApiState>,
    session: Session,
    Json(body): Json<ProjectBatchRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.batch";
    let (ids, action) = match body {
        ProjectBatchRequest::Archive { ids } => (ids, ProjectBatchAction::Archive),
        ProjectBatchRequest::Unarchive { ids } => (ids, ProjectBatchAction::Unarchive),
        ProjectBatchRequest::Delete { ids } => (ids, ProjectBatchAction::Delete),
    };
    let ids = normalize_ids(ids, OP)?;
    let results = run(ids, |project_id| {
        let state = state.clone();
        let session = session.clone();
        async move {
            match action {
                ProjectBatchAction::Archive => {
                    crate::domain::admin_projects::archive(&state, session.data.user_id, project_id)
                        .await
                        .map(|_| ())
                }
                ProjectBatchAction::Unarchive => crate::domain::admin_projects::unarchive(
                    &state,
                    session.data.user_id,
                    project_id,
                )
                .await
                .map(|_| ()),
                ProjectBatchAction::Delete => {
                    crate::domain::admin_projects::remove(&state, session.data.user_id, project_id)
                        .await
                        .map(|_| ())
                }
            }
        }
    })
    .await;

    Ok(ok_response(BatchResponse { results }))
}

#[derive(serde::Serialize)]
struct BatchResponse {
    results: Vec<BatchItemResult>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn project_batch_actions_are_resource_specific() {
        let id = Uuid::now_v7();
        let request: ProjectBatchRequest = serde_json::from_value(serde_json::json!({
            "action": "archive",
            "ids": [id],
        }))
        .unwrap();
        assert!(matches!(request, ProjectBatchRequest::Archive { .. }));
        assert!(
            serde_json::from_value::<ProjectBatchRequest>(serde_json::json!({
                "action": "restore",
                "ids": [id],
            }))
            .is_err()
        );
    }
}
