use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{
        delivery::{self, ReleaseRequestOutcome},
        deployments::{self, DeploymentStateError},
        projects, scheduler,
    },
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{
            AuditEventResult, AuditEventVisibility, DeploymentBuildStatus, DeploymentEventKind,
            DeploymentReleaseStatus, DeploymentServeStatus, ReleaseReason,
        },
        error::{AppError, accepted_response, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/deployments/{deployment_id}/review/reject",
        axum::routing::post(reject),
    )
}

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

#[derive(Deserialize, Default)]
struct DecisionRequest {
    #[serde(default)]
    reason: Option<String>,
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
    let db = crate::infra::http::database(&state, op)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    if approved {
        scheduler::lock_placement(&transaction)
            .await
            .map_err(|error| {
                crate::infra::http::deployment_errors::map_delivery_error(
                    delivery::DeliveryError::Schedule(error),
                    op,
                )
            })?;
    }
    let deployment = deployments::get_by_id_for_update(&transaction, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "deployment not found".to_owned(),
        })?;
    let project = projects::get_by_id_any(&transaction, deployment.project_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "project not found".to_owned(),
        })?;

    if !matches!(deployment.build_status, DeploymentBuildStatus::Ready)
        || !matches!(
            deployment.serve_status,
            DeploymentServeStatus::Ready | DeploymentServeStatus::Retired
        )
    {
        return Err(AppError::Conflict {
            op,
            message: "only deployments with ready build and Serve artifact can be reviewed"
                .to_owned(),
        });
    }
    if matches!(deployment.serve_status, DeploymentServeStatus::Retired)
        && !deployments::was_serve_ready(&transaction, deployment.id)
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?
    {
        return Err(AppError::Conflict {
            op,
            message: "retired deployment never reached Serve Ready".to_owned(),
        });
    }

    let review = deployments::latest_pending_review(&transaction, deployment.id)
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
    let review = deployments::resolve_review(
        &transaction,
        review,
        session.data.user_id,
        approved,
        reason.clone(),
    )
    .await
    .map_err(|source| AppError::Infrastructure { op, source })?;

    let target_status = if approved {
        DeploymentReleaseStatus::Approved
    } else {
        DeploymentReleaseStatus::Rejected
    };
    let mut deployment = deployments::transition_release(
        &transaction,
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
        &transaction,
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

    audits::create_platform_audit_event(
        &transaction,
        CreateAuditEventParams {
            actor_user_id: Some(session.data.user_id),
            actor_node_id: None,
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
    .await
    .map_err(|source| AppError::Infrastructure { op, source })?;

    let mut release_pending = false;
    if approved {
        let outcome = delivery::request_release(
            &transaction,
            deployment,
            ReleaseReason::Promote,
            session.data.user_id,
            AuditEventVisibility::Platform,
        )
        .await
        .map_err(|error| crate::infra::http::deployment_errors::map_delivery_error(error, op))?;
        (deployment, release_pending) = match outcome {
            ReleaseRequestOutcome::Activated(deployment) => (deployment, false),
            ReleaseRequestOutcome::SyncQueued(deployment) => (deployment, true),
        };
        audits::create_platform_audit_event(
            &transaction,
            CreateAuditEventParams {
                actor_user_id: Some(session.data.user_id),
                actor_node_id: None,
                team_id: Some(deployment.team_id),
                action: delivery::release_audit_action(&ReleaseReason::Promote, release_pending)
                    .to_owned(),
                target_type: "deployment".to_owned(),
                target_id: Some(deployment.id),
                result: AuditEventResult::Success,
                reason: None,
                metadata: json!({
                    "platform_admin": true,
                    "project_id": project.id,
                    "release_pending": release_pending,
                }),
            },
        )
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    }
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;

    let response = DecisionResponse {
        deployment_id: deployment.id,
        release_status: deployments::release_status_value(&deployment.release_status),
        release_pending,
        review: ReviewDecisionResponse {
            id: review.id,
            status: if approved { "approved" } else { "rejected" },
            reason: review.reason,
        },
    };
    Ok(if release_pending {
        accepted_response(response).into_response()
    } else {
        ok_response(response).into_response()
    })
}

/// POST /api/v1/admin/deployments/{deployment_id}/review/reject
async fn reject(
    State(state): State<ControlApiState>,
    session: Session,
    Path(deployment_id): Path<Uuid>,
    body: Option<Json<DecisionRequest>>,
) -> Result<impl IntoResponse, AppError> {
    let body = body.map(|Json(body)| body).unwrap_or_default();
    decide(state, session, deployment_id, false, body).await
}

#[derive(serde::Serialize)]
struct DecisionResponse {
    deployment_id: Uuid,
    release_status: &'static str,
    release_pending: bool,
    review: ReviewDecisionResponse,
}

#[derive(serde::Serialize)]
struct ReviewDecisionResponse {
    id: Uuid,
    status: &'static str,
    reason: Option<String>,
}
