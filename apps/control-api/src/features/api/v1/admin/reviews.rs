//! Platform-wide release review queue. Platform administrators see every
//! pending review and decide here, optionally promoting production
//! deployments in the same step.

use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, TransactionTrait};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::http::timestamps::ts;
use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        deployments::{self, DeploymentStateError},
        projects,
    },
    infra::{
        database::entity::{
            AuditEventResult, DeploymentBuildStatus, DeploymentEventKind, DeploymentReleaseStatus,
            DeploymentReviewStatus, DeploymentServeStatus, ReleaseReason, deployment,
            deployment_review, project, team, user,
        },
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

fn map_state_error(error: DeploymentStateError, op: &'static str) -> AppError {
    match error {
        DeploymentStateError::Database(source) => AppError::Infrastructure {
            op,
            source: source.into(),
        },
        other => AppError::Conflict {
            op,
            message: other.to_string(),
        },
    }
}

/// GET /api/v1/admin/reviews — pending release reviews across all teams,
/// oldest first.
pub async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.reviews.list";
    let db = super::database(&state, OP)?;

    let reviews = deployment_review::Entity::find()
        .filter(deployment_review::Column::Status.eq(DeploymentReviewStatus::Pending))
        .order_by_asc(deployment_review::Column::RequestedAt)
        .limit(200)
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    let deployment_ids: Vec<Uuid> = reviews.iter().map(|review| review.deployment_id).collect();
    let deployments_map: HashMap<Uuid, deployment::Model> = if deployment_ids.is_empty() {
        HashMap::new()
    } else {
        deployment::Entity::find()
            .filter(deployment::Column::Id.is_in(deployment_ids))
            .filter(deployment::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .into_iter()
            .map(|deployment| (deployment.id, deployment))
            .collect()
    };

    let project_ids: Vec<Uuid> = deployments_map
        .values()
        .map(|deployment| deployment.project_id)
        .collect();
    let projects_map: HashMap<Uuid, project::Model> = if project_ids.is_empty() {
        HashMap::new()
    } else {
        project::Entity::find()
            .filter(project::Column::Id.is_in(project_ids))
            .all(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .into_iter()
            .map(|project| (project.id, project))
            .collect()
    };

    let team_ids: Vec<Uuid> = deployments_map
        .values()
        .map(|deployment| deployment.team_id)
        .collect();
    let teams_map: HashMap<Uuid, team::Model> = if team_ids.is_empty() {
        HashMap::new()
    } else {
        team::Entity::find()
            .filter(team::Column::Id.is_in(team_ids))
            .all(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .into_iter()
            .map(|team| (team.id, team))
            .collect()
    };

    let user_ids: Vec<Uuid> = deployments_map
        .values()
        .filter_map(|deployment| deployment.triggered_by_user_id)
        .collect();
    let users_map: HashMap<Uuid, user::Model> = if user_ids.is_empty() {
        HashMap::new()
    } else {
        user::Entity::find()
            .filter(user::Column::Id.is_in(user_ids))
            .all(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .into_iter()
            .map(|user| (user.id, user))
            .collect()
    };

    let rows: Vec<serde_json::Value> = reviews
        .iter()
        .filter_map(|review| {
            let deployment = deployments_map.get(&review.deployment_id)?;
            let project = projects_map.get(&deployment.project_id)?;
            let team = teams_map.get(&deployment.team_id);
            let triggered_by = deployment
                .triggered_by_user_id
                .and_then(|id| users_map.get(&id));
            Some(json!({
                "id": review.id,
                "requested_at": ts(review.requested_at),
                "deployment": {
                    "id": deployment.id,
                    "environment": deployments::environment_value(&deployment.environment),
                    "build_status": deployments::build_status_value(&deployment.build_status),
                    "serve_status": deployments::serve_status_value(&deployment.serve_status),
                    "release_status": deployments::release_status_value(&deployment.release_status),
                    "source_branch": deployment.source_branch,
                    "commit_hash": deployment.commit_hash,
                    "commit_message": deployment.commit_message,
                    "preview_host": deployment.preview_host,
                    "created_at": ts(deployment.created_at),
                },
                "project": {
                    "id": project.id,
                    "name": project.name,
                    "slug": project.slug,
                },
                "team": team.map(|team| json!({
                    "id": team.id,
                    "name": team.name,
                    "slug": team.slug,
                })),
                "triggered_by": triggered_by.map(|user| json!({
                    "id": user.id,
                    "email": user.email,
                    "display_name": user.display_name,
                })),
            }))
        })
        .collect();

    Ok(ok_response(json!({
        "total": rows.len(),
        "reviews": rows,
    })))
}

#[derive(Deserialize, Default)]
pub struct DecisionRequest {
    #[serde(default)]
    pub reason: Option<String>,
    /// Approve only: also activate the deployment in the same step.
    #[serde(default)]
    pub promote: bool,
}

async fn decide(
    state: ControlApiState,
    session: Session,
    deployment_id: Uuid,
    approved: bool,
    body: DecisionRequest,
) -> Result<axum::response::Response, AppError> {
    let op: &'static str = if approved {
        "admin.reviews.approve"
    } else {
        "admin.reviews.reject"
    };
    let db = super::database(&state, op)?;

    let deployment = deployments::get_by_id(db, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "deployment not found".to_owned(),
        })?;
    let project = projects::get_by_id_any(db, deployment.project_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "project not found".to_owned(),
        })?;

    if !matches!(deployment.build_status, DeploymentBuildStatus::Ready)
        || !matches!(deployment.serve_status, DeploymentServeStatus::Ready)
    {
        return Err(AppError::Conflict {
            op,
            message: "only deployments with ready build and Serve artifact can be reviewed"
                .to_owned(),
        });
    }

    let review = deployments::latest_pending_review(db, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::Conflict {
            op,
            message: "deployment has no pending review".to_owned(),
        })?;

    let reason = body.reason.and_then(|value| {
        let trimmed = value.trim().to_owned();
        (!trimmed.is_empty()).then_some(trimmed)
    });
    let review =
        deployments::resolve_review(db, review, session.data.user_id, approved, reason.clone())
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?;

    let target_status = if approved {
        DeploymentReleaseStatus::Approved
    } else {
        DeploymentReleaseStatus::Rejected
    };
    let mut deployment = deployments::transition_release(
        db,
        deployment,
        target_status,
        json!({
            "review_id": review.id,
            "reviewer": session.data.user_id,
            "platform_admin": true,
            "reason": reason,
        }),
    )
    .await
    .map_err(|error| map_state_error(error, op))?;

    deployments::append_event(
        db,
        deployment.id,
        DeploymentEventKind::Review,
        if approved {
            "review approved by platform administrator"
        } else {
            "review rejected by platform administrator"
        },
        json!({ "review_id": review.id, "reason": review.reason }),
    )
    .await
    .map_err(|source| AppError::Infrastructure { op, source })?;

    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(session.data.user_id),
            team_id: Some(deployment.team_id),
            action: if approved {
                "deployment.review_approved".to_owned()
            } else {
                "deployment.review_rejected".to_owned()
            },
            target_type: "deployment".to_owned(),
            target_id: Some(deployment.id),
            result: AuditEventResult::Success,
            reason: review.reason.clone(),
            metadata: json!({ "platform_admin": true, "project_id": project.id }),
        },
    )
    .await;

    let mut promoted = false;
    if approved && body.promote {
        let transaction = db
            .begin()
            .await
            .map_err(|source| AppError::Infrastructure {
                op,
                source: source.into(),
            })?;
        deployment = deployments::activate(
            &transaction,
            deployment,
            ReleaseReason::Promote,
            Some(session.data.user_id),
        )
        .await
        .map_err(|error| map_state_error(error, op))?;
        transaction
            .commit()
            .await
            .map_err(|source| AppError::Infrastructure {
                op,
                source: source.into(),
            })?;
        promoted = true;

        let _ = audits::create_audit_event(
            db,
            CreateAuditEventParams {
                actor_user_id: Some(session.data.user_id),
                team_id: Some(deployment.team_id),
                action: "deployment.promoted".to_owned(),
                target_type: "deployment".to_owned(),
                target_id: Some(deployment.id),
                result: AuditEventResult::Success,
                reason: None,
                metadata: json!({ "platform_admin": true, "project_id": project.id }),
            },
        )
        .await;
    }

    Ok(ok_response(json!({
        "deployment_id": deployment.id,
        "release_status": deployments::release_status_value(&deployment.release_status),
        "promoted": promoted,
        "review": {
            "id": review.id,
            "status": if approved { "approved" } else { "rejected" },
            "reason": review.reason,
        },
    }))
    .into_response())
}

/// POST /api/v1/admin/deployments/{deployment_id}/review/approve
pub async fn approve(
    State(state): State<ControlApiState>,
    session: Session,
    Path(deployment_id): Path<Uuid>,
    body: Option<Json<DecisionRequest>>,
) -> Result<impl IntoResponse, AppError> {
    let body = body.map(|Json(body)| body).unwrap_or_default();
    decide(state, session, deployment_id, true, body).await
}

/// POST /api/v1/admin/deployments/{deployment_id}/review/reject
pub async fn reject(
    State(state): State<ControlApiState>,
    session: Session,
    Path(deployment_id): Path<Uuid>,
    body: Option<Json<DecisionRequest>>,
) -> Result<impl IntoResponse, AppError> {
    let body = body.map(|Json(body)| body).unwrap_or_default();
    decide(state, session, deployment_id, false, body).await
}
