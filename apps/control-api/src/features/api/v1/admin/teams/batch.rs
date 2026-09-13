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
    axum::Router::new().route("/teams/batch", axum::routing::post(batch))
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
pub enum TeamBatchRequest {
    Delete {
        ids: Vec<Uuid>,
    },
    AssignGroup {
        ids: Vec<Uuid>,
        group_id: Uuid,
    },
    AssignQuotaPlan {
        ids: Vec<Uuid>,
        plan_id: Option<Uuid>,
    },
}

#[derive(Clone, Copy)]
enum TeamBatchAction {
    Delete,
    AssignGroup(Uuid),
    AssignQuotaPlan(Option<Uuid>),
}

/// POST /api/v1/admin/teams/batch
pub async fn batch(
    State(state): State<ControlApiState>,
    session: Session,
    Json(body): Json<TeamBatchRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.teams.batch";
    let (ids, action) = match body {
        TeamBatchRequest::Delete { ids } => (ids, TeamBatchAction::Delete),
        TeamBatchRequest::AssignGroup { ids, group_id } => {
            (ids, TeamBatchAction::AssignGroup(group_id))
        }
        TeamBatchRequest::AssignQuotaPlan { ids, plan_id } => {
            (ids, TeamBatchAction::AssignQuotaPlan(plan_id))
        }
    };
    let ids = normalize_ids(ids, OP)?;
    let results = run(ids, |team_id| {
        let state = state.clone();
        let session = session.clone();
        async move {
            match action {
                TeamBatchAction::Delete => {
                    crate::domain::admin_teams::remove(&state, session.data.user_id, team_id)
                        .await
                        .map(|_| ())
                }
                TeamBatchAction::AssignGroup(group_id) => crate::domain::admin_team_groups::assign(
                    &state,
                    session.data.user_id,
                    team_id,
                    group_id,
                )
                .await
                .map(|_| ()),
                TeamBatchAction::AssignQuotaPlan(plan_id) => {
                    crate::domain::admin_teams::set_quota_plan(
                        &state,
                        session.data.user_id,
                        team_id,
                        plan_id,
                    )
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

    use serde_json::json;
    use uuid::Uuid;
    #[test]
    fn team_batch_actions_require_their_payloads() {
        let id = Uuid::now_v7();
        let group_id = Uuid::now_v7();
        let request: TeamBatchRequest = serde_json::from_value(json!({
            "action": "assign_group",
            "ids": [id],
            "group_id": group_id,
        }))
        .unwrap();
        assert!(matches!(request, TeamBatchRequest::AssignGroup { .. }));
        assert!(
            serde_json::from_value::<TeamBatchRequest>(json!({
                "action": "assign_group",
                "ids": [id],
            }))
            .is_err()
        );
    }
}
