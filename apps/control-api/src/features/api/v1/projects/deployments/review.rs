use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::TransactionTrait;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        deployments,
    },
    infra::{
        database::entity::{
            AuditEventResult, DeploymentBuildStatus, DeploymentEventKind, DeploymentReleaseStatus,
        },
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

use super::map_state_error;

async fn record_review_audit(
    db: &sea_orm::DatabaseConnection,
    actor: Uuid,
    team_id: Uuid,
    action: &str,
    deployment_id: Uuid,
    reason: Option<String>,
) {
    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(actor),
            team_id: Some(team_id),
            action: action.to_owned(),
            target_type: "deployment".to_owned(),
            target_id: Some(deployment_id),
            result: AuditEventResult::Success,
            reason,
            metadata: json!({}),
        },
    )
    .await;
}

async fn load_deployment(
    state: &ControlApiState,
    session: &Session,
    project_id: Uuid,
    deployment_id: Uuid,
    op: &'static str,
) -> Result<
    (
        crate::features::api::v1::projects::ProjectAccess,
        crate::infra::database::entity::deployment::Model,
    ),
    AppError,
> {
    let access =
        crate::features::api::v1::projects::project_access(state, session, project_id, false, op)
            .await?;
    let db = crate::features::api::v1::projects::database(state, op)?;
    let deployment = deployments::get_by_id(db, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .filter(|deployment| deployment.project_id == access.project.id)
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "deployment not found".to_owned(),
        })?;
    Ok((access, deployment))
}

/// POST /api/v1/projects/{project_id}/deployments/{deployment_id}/review/request
pub async fn request(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.review.request";
    let (access, deployment) =
        load_deployment(&state, &session, project_id, deployment_id, OP).await?;
    access.require_member(OP)?;
    let db = crate::features::api::v1::projects::database(&state, OP)?;

    if !matches!(deployment.build_status, DeploymentBuildStatus::Ready) {
        return Err(AppError::Conflict {
            op: OP,
            message: "only ready deployments can request review".to_owned(),
        });
    }

    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let deployment = deployments::get_by_id_for_update(&transaction, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment not found".to_owned(),
        })?;
    let (deployment, review) =
        deployments::request_review(&transaction, deployment, Some(session.data.user_id))
            .await
            .map_err(|error| map_state_error(error, OP))?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    record_review_audit(
        db,
        session.data.user_id,
        access.team.id,
        "deployment.review_requested",
        deployment.id,
        None,
    )
    .await;

    Ok(ok_response(json!({
        "deployment_id": deployment.id,
        "release_status": deployments::release_status_value(&deployment.release_status),
        "review": { "id": review.id, "status": "pending" },
    })))
}

#[derive(Deserialize)]
pub struct ReviewDecisionRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

async fn decide(
    state: ControlApiState,
    session: Session,
    project_id: Uuid,
    deployment_id: Uuid,
    approved: bool,
    reason: Option<String>,
) -> Result<axum::response::Response, AppError> {
    let op: &'static str = if approved {
        "deployments.review.approve"
    } else {
        "deployments.review.reject"
    };
    let (access, deployment) =
        load_deployment(&state, &session, project_id, deployment_id, op).await?;
    // Approvals are a governance action: team admin or owner only.
    access.require_admin(op)?;
    let db = crate::features::api::v1::projects::database(&state, op)?;

    let review = deployments::latest_pending_review(db, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::Conflict {
            op,
            message: "deployment has no pending review".to_owned(),
        })?;

    let review =
        deployments::resolve_review(db, review, session.data.user_id, approved, reason.clone())
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?;

    let target_status = if approved {
        DeploymentReleaseStatus::Approved
    } else {
        DeploymentReleaseStatus::Rejected
    };
    let deployment = deployments::transition_release(
        db,
        deployment,
        target_status,
        json!({
            "review_id": review.id,
            "reviewer": session.data.user_id,
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
            "review approved"
        } else {
            "review rejected"
        },
        json!({ "review_id": review.id, "reason": review.reason }),
    )
    .await
    .map_err(|source| AppError::Infrastructure { op, source })?;
    record_review_audit(
        db,
        session.data.user_id,
        access.team.id,
        if approved {
            "deployment.review_approved"
        } else {
            "deployment.review_rejected"
        },
        deployment.id,
        review.reason.clone(),
    )
    .await;

    Ok(ok_response(json!({
        "deployment_id": deployment.id,
        "release_status": deployments::release_status_value(&deployment.release_status),
        "review": {
            "id": review.id,
            "status": if approved { "approved" } else { "rejected" },
            "reason": review.reason,
        },
    }))
    .into_response())
}

/// POST /api/v1/projects/{project_id}/deployments/{deployment_id}/review/approve
pub async fn approve(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ReviewDecisionRequest>,
) -> Result<impl IntoResponse, AppError> {
    decide(state, session, project_id, deployment_id, true, body.reason).await
}

/// POST /api/v1/projects/{project_id}/deployments/{deployment_id}/review/reject
pub async fn reject(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ReviewDecisionRequest>,
) -> Result<impl IntoResponse, AppError> {
    decide(
        state,
        session,
        project_id,
        deployment_id,
        false,
        body.reason,
    )
    .await
}
