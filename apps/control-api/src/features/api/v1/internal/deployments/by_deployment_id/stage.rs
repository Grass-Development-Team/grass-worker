use axum::{
    Extension, Json,
    extract::{Path, State},
    response::IntoResponse,
};
use grass_cache::Cache;
use grass_node_protocol::ReportedStatus;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{
        delivery,
        deployment_cancellation::cancel_flag_key,
        deployments::{self, BuildTransition, ReadyReleaseAction},
        platform_mail,
        quotas::QuotaDimension,
        scheduler,
    },
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{
            AuditEventResult, DeploymentArtifactKind, DeploymentBuildStatus, ReleaseReason,
            deployment, deployment_artifact, node,
        },
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
        quota::{QuotaCharge, QuotaService},
        storage::StorageManager,
    },
    state::ControlApiState,
};

// --- Stage reports ----------------------------------------------------------
/// A build status/stage report from the Node. `status: None` reports a stage
/// change inside the current status (for example install → build while
/// `building`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StageRequest {
    #[serde(default)]
    status: Option<ReportedStatus>,
    #[serde(default)]
    stage: Option<String>,
    #[serde(default)]
    failure_code: Option<String>,
    #[serde(default)]
    failure_message: Option<String>,
    /// Whole build minutes consumed, reported once with the terminal status.
    #[serde(default)]
    build_minutes: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StageResponse {
    /// The server asks the Node to stop this build (user cancel).
    cancel_requested: bool,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route(
        "/deployments/{deployment_id}/stage",
        axum::routing::post(stage),
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

// --- Stage reports ----------------------------------------------------------
/// POST /api/v1/internal/deployments/{deployment_id}/stage
async fn stage(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
    Json(body): Json<StageRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.deployments.stage";
    let db = crate::infra::http::database(&state, OP)?;
    let cache = crate::infra::http::cache(&state, OP)?;
    let deployment = build_owned_deployment(db, &node, deployment_id, OP).await?;
    let quota = QuotaService::new(db, cache);

    // A user cancel wins over any progress report: tell the Node to stop.
    let cancel_flagged = cache
        .get(&cancel_flag_key(deployment_id))
        .await
        .ok()
        .flatten()
        .is_some();
    if crate::domain::deployment_cancellation::cancellation_was_requested(
        &deployment.build_status,
        cancel_flagged,
    ) {
        // The Node acknowledges by reporting canceled; account build minutes
        // when it does. The concurrency slot was already released by
        // cancel_deployment_core when the cancel was issued, so we must not
        // release it again here or the team's slot count underflows.
        if matches!(body.status, Some(ReportedStatus::Canceled)) {
            let _ = cache.delete(&cancel_flag_key(deployment_id)).await;
            if let Some(minutes) = body.build_minutes.filter(|minutes| *minutes > 0) {
                quota
                    .charge_unchecked(
                        OP,
                        deployment.team_id,
                        &[QuotaCharge::amount(
                            QuotaDimension::BuildMinutesMonthly,
                            minutes,
                        )],
                        "deployment",
                        Some(deployment.id),
                    )
                    .await?;
            }
        }
        return Ok(ok_response(StageResponse {
            cancel_requested: true,
        }));
    }

    quota.refresh_build_slot(deployment.team_id).await;

    let response = match body.status {
        None => {
            if let Some(stage) = &body.stage {
                deployments::update_stage(db, deployment, stage)
                    .await
                    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
            }
            StageResponse {
                cancel_requested: false,
            }
        }
        Some(status) => {
            let target = match status {
                ReportedStatus::Queued => DeploymentBuildStatus::Queued,
                ReportedStatus::Building => DeploymentBuildStatus::Building,
                ReportedStatus::Ready => DeploymentBuildStatus::Ready,
                ReportedStatus::Failed => DeploymentBuildStatus::Failed,
                ReportedStatus::Canceled => DeploymentBuildStatus::Canceled,
            };
            let team_id = deployment.team_id;
            let is_terminal = matches!(
                target,
                DeploymentBuildStatus::Ready
                    | DeploymentBuildStatus::Failed
                    | DeploymentBuildStatus::Canceled
            );

            let was_started = matches!(status, ReportedStatus::Building)
                && !matches!(deployment.build_status, DeploymentBuildStatus::Building);
            let transition = BuildTransition {
                to: target.clone(),
                stage: body.stage.clone(),
                failure_code: body.failure_code.clone(),
                failure_message: body.failure_message.clone(),
                build_node_id: Some(node.id),
            };
            let transaction = audits::AuditTransaction::begin(db)
                .await
                .map_err(|source| AppError::Infrastructure {
                    op: OP,
                    source: source.into(),
                })?;
            let (updated, build_transitioned, ready_action) =
                if matches!(target, DeploymentBuildStatus::Ready) {
                    finalize_ready(&transaction, deployment, transition).await?
                } else if matches!(
                    target,
                    DeploymentBuildStatus::Failed | DeploymentBuildStatus::Canceled
                ) {
                    let updated = delivery::transition_unsuccessful_build(
                        &transaction,
                        deployment,
                        transition,
                    )
                    .await
                    .map_err(|error| {
                        crate::infra::http::deployment_errors::map_delivery_error(error, OP)
                    })?;

                    (updated, true, ReadyReleaseAction::None)
                } else {
                    let updated =
                        deployments::transition_build(&transaction, deployment, transition)
                            .await
                            .map_err(|error| {
                                crate::infra::http::deployment_errors::map_state_error(error, OP)
                            })?;
                    (updated, true, ReadyReleaseAction::None)
                };

            if was_started {
                audits::create_audit_event(
                    &transaction,
                    CreateAuditEventParams {
                        actor_user_id: None,
                        actor_node_id: Some(node.id),
                        team_id: Some(team_id),
                        action: "deployment.build_started".to_owned(),
                        target_type: "deployment".to_owned(),
                        target_id: Some(updated.id),
                        result: AuditEventResult::Success,
                        reason: None,
                        metadata: json!({ "build_node_id": node.id }),
                    },
                )
                .await
                .map_err(|source| AppError::Infrastructure { op: OP, source })?;
            }

            if is_terminal && build_transitioned {
                audits::create_audit_event(
                    &transaction,
                    CreateAuditEventParams {
                        actor_user_id: None,
                        actor_node_id: Some(node.id),
                        team_id: Some(team_id),
                        action: "deployment.build_finished".to_owned(),
                        target_type: "deployment".to_owned(),
                        target_id: Some(updated.id),
                        result: if matches!(target, DeploymentBuildStatus::Ready) {
                            AuditEventResult::Success
                        } else {
                            AuditEventResult::Failure
                        },
                        reason: body.failure_message.clone(),
                        metadata: json!({
                            "status": deployments::build_status_value(&target),
                        }),
                    },
                )
                .await
                .map_err(|source| AppError::Infrastructure { op: OP, source })?;
            }

            if matches!(ready_action, ReadyReleaseAction::RequestReview) {
                audits::create_audit_event(
                    &transaction,
                    CreateAuditEventParams {
                        actor_user_id: None,
                        actor_node_id: None,
                        team_id: Some(team_id),
                        action: "deployment.review_requested".to_owned(),
                        target_type: "deployment".to_owned(),
                        target_id: Some(updated.id),
                        result: AuditEventResult::Success,
                        reason: None,
                        metadata: json!({ "automatic": true }),
                    },
                )
                .await
                .map_err(|source| AppError::Infrastructure { op: OP, source })?;
            }

            transaction
                .commit()
                .await
                .map_err(|source| AppError::Infrastructure {
                    op: OP,
                    source: source.into(),
                })?;
            if is_terminal && build_transitioned {
                quota.release_build_slot_once(team_id, updated.id).await;
                if let Some(minutes) = body.build_minutes.filter(|minutes| *minutes > 0) {
                    quota
                        .charge_unchecked(
                            OP,
                            team_id,
                            &[QuotaCharge::amount(
                                QuotaDimension::BuildMinutesMonthly,
                                minutes,
                            )],
                            "deployment",
                            Some(updated.id),
                        )
                        .await?;
                }

                // Record the persisted build log as an artifact row once.
                if state
                    .storage
                    .clone()
                    .read_build_log(updated.project_id, updated.id)
                    .await
                    .ok()
                    .flatten()
                    .is_some()
                {
                    let _ = record_build_log_artifact(db, &updated).await;
                }

                let mail_config = state.config.read().unwrap().mail.clone();
                platform_mail::send_deployment_result_best_effort(db, mail_config, &updated).await;
            }

            StageResponse {
                cancel_requested: false,
            }
        }
    };

    Ok(ok_response(response))
}

async fn record_build_log_artifact(
    db: &sea_orm::DatabaseConnection,
    deployment: &deployment::Model,
) -> anyhow::Result<()> {
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};

    let existing = deployment_artifact::Entity::find()
        .filter(deployment_artifact::Column::DeploymentId.eq(deployment.id))
        .filter(deployment_artifact::Column::Kind.eq(DeploymentArtifactKind::BuildLog))
        .filter(deployment_artifact::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    if existing.is_some() {
        return Ok(());
    }

    deployment_artifact::ActiveModel {
        id: Set(Uuid::now_v7()),
        deployment_id: Set(deployment.id),
        kind: Set(DeploymentArtifactKind::BuildLog),
        storage_path: Set(StorageManager::build_log_relative_path(
            deployment.project_id,
            deployment.id,
        )),
        checksum_sha256: Set(None),
        size_bytes: Set(None),
        manifest: Set(json!({})),
        deleted_at: Set(None),
        created_at: Set(time::OffsetDateTime::now_utc()),
    }
    .insert(db)
    .await?;
    Ok(())
}

/// Commits a successful build and its initial release state atomically.
/// Repeated Ready reports only repair historical Ready/Draft rows and do not
/// repeat terminal accounting side effects.
fn ready_report_needs_build_transition(status: &DeploymentBuildStatus) -> bool {
    !matches!(status, DeploymentBuildStatus::Ready)
}

async fn finalize_ready(
    transaction: &audits::AuditTransaction,
    requested_deployment: deployment::Model,
    transition: BuildTransition,
) -> Result<(deployment::Model, bool, ReadyReleaseAction), AppError> {
    const OP: &str = "internal.deployments.finalize_ready";

    scheduler::lock_placement(transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    // Serialize retries for this deployment and make the decision from the
    // latest row, not from the pre-transaction request snapshot.
    let deployment = deployments::get_by_id_for_update(transaction, requested_deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment not found".to_owned(),
        })?;
    let build_transitioned = ready_report_needs_build_transition(&deployment.build_status);
    let deployment = if build_transitioned {
        deployments::transition_build(transaction, deployment, transition)
            .await
            .map_err(|error| crate::infra::http::deployment_errors::map_state_error(error, OP))?
    } else {
        deployment
    };
    let policy = deployments::review_policy_for_team(transaction, deployment.team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let action = deployments::ready_release_action(
        policy.mode_for(&deployment.environment),
        &deployment.release_status,
    );
    let deployment = match action {
        ReadyReleaseAction::Activate => {
            deployments::activate(transaction, deployment, ReleaseReason::Auto, None)
                .await
                .map_err(|error| {
                    crate::infra::http::deployment_errors::map_state_error(error, OP)
                })?
        }
        ReadyReleaseAction::RequestReview => {
            deployments::request_review(transaction, deployment, None)
                .await
                .map_err(|error| crate::infra::http::deployment_errors::map_state_error(error, OP))?
                .0
        }
        ReadyReleaseAction::None => deployment,
    };
    delivery::reconcile_environment(
        transaction,
        deployment.project_id,
        deployment.environment.clone(),
    )
    .await
    .map_err(|source| AppError::Infrastructure {
        op: OP,
        source: source.into(),
    })?;

    Ok((deployment, build_transitioned, action))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_ready_reports_skip_the_build_transition() {
        assert!(ready_report_needs_build_transition(
            &DeploymentBuildStatus::Building
        ));
        assert!(!ready_report_needs_build_transition(
            &DeploymentBuildStatus::Ready
        ));
    }

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<StageRequest, grass_node_protocol::StageRequest>(
            "StageRequest",
        );
        crate::test_support::assert_node_contract::<
            StageResponse,
            grass_node_protocol::StageResponse,
        >("StageResponse");
    }
}
