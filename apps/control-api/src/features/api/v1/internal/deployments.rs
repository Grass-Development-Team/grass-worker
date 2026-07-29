use axum::{
    Extension, Json,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
};
use grass_cache::Cache;
use grass_node_protocol::{
    AppendBuildLogRequest, AppendBuildLogResponse, ClaimRequest, ClaimResponse, ClaimedDeployment,
    ObserveSshHostKeyRequest, ObserveSshHostKeyResponse, RedeemGitCredentialRequest,
    RedeemGitCredentialResponse, ReportedStatus, StageRequest, StageResponse,
    UploadArtifactResponse, artifact_headers,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, TransactionTrait,
};
use serde_json::json;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        delivery,
        deployments::{self, BuildTransition, ReadyReleaseAction},
        quotas::QuotaDimension,
        scheduler::{self, NodeUsage},
        source_credentials, ssh_host_keys, teams,
    },
    infra::{
        database::entity::{
            AuditEventResult, DeploymentArtifactKind, DeploymentBuildStatus, ProjectRuntime,
            ReleaseReason, deployment, deployment_artifact, node, team,
        },
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
        quota::{QuotaCharge, QuotaService},
        storage::{LocalStorage, StorageError},
    },
    state::ControlApiState,
};

fn cancel_flag_key(deployment_id: Uuid) -> String {
    crate::features::api::v1::projects::deployments::cancel_flag_key(deployment_id)
}

async fn team_for(
    db: &sea_orm::DatabaseConnection,
    team_id: Uuid,
    op: &'static str,
) -> Result<team::Model, AppError> {
    teams::get_by_id(db, team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "team not found".to_owned(),
        })
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

// --- Claim ------------------------------------------------------------------

/// POST /api/v1/internal/deployments/claim
pub async fn claim(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Json(body): Json<ClaimRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.deployments.claim";
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;

    if !node.build_enabled {
        return Err(AppError::Forbidden {
            op: OP,
            message: "node does not have build capability".to_owned(),
        });
    }

    if body.capacity == 0 {
        return Ok(ok_response(ClaimResponse { deployment: None }));
    }

    // Oldest pending static deployments first. Non-static runtimes never
    // reach the queue; they fail at creation time.
    let candidates = deployment::Entity::find()
        .filter(deployment::Column::BuildStatus.eq(DeploymentBuildStatus::Pending))
        .filter(
            deployment::Column::RuntimeKind.is_in([ProjectRuntime::Static, ProjectRuntime::Ssr]),
        )
        .filter(deployment::Column::DeletedAt.is_null())
        .order_by_asc(deployment::Column::CreatedAt)
        .limit(10)
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    let quota = QuotaService::new(db, cache);
    for candidate in candidates {
        let team = team_for(db, candidate.team_id, OP).await?;

        // Concurrency slot first so we never claim more than the team may
        // run; released on every failure path below so a transient error
        // cannot leave the team blocked until the slot TTL expires.
        if !quota.acquire_build_slot(OP, &team).await? {
            continue;
        }

        let build_timeout_seconds = match quota
            .scalar_limit(OP, &team, QuotaDimension::BuildTimeoutSeconds)
            .await
        {
            Ok(limit) => limit,
            Err(error) => {
                quota.release_build_slot(team.id).await;
                return Err(error);
            }
        };

        // Optimistic claim: only one node can flip pending → claimed.
        let claim_result = deployment::Entity::update_many()
            .col_expr(
                deployment::Column::BuildStatus,
                sea_orm::ActiveEnum::as_enum(&DeploymentBuildStatus::Claimed),
            )
            .col_expr(
                deployment::Column::BuildNodeId,
                sea_orm::sea_query::Expr::value(node.id),
            )
            .col_expr(
                deployment::Column::ClaimedAt,
                sea_orm::sea_query::Expr::value(time::OffsetDateTime::now_utc()),
            )
            .filter(deployment::Column::Id.eq(candidate.id))
            .filter(deployment::Column::BuildStatus.eq(DeploymentBuildStatus::Pending))
            .exec(db)
            .await;
        let claim_result = match claim_result {
            Ok(result) => result,
            Err(source) => {
                quota.release_build_slot(team.id).await;
                return Err(AppError::Infrastructure {
                    op: OP,
                    source: source.into(),
                });
            }
        };

        if claim_result.rows_affected == 0 {
            quota.release_build_slot(team.id).await;
            continue;
        }

        let source_credential_lease = match candidate.source_credential_version_id {
            Some(version_id) => {
                match source_credentials::issue_lease(db, node.id, candidate.id, version_id).await {
                    Ok(lease) => Some(lease),
                    Err(error) => {
                        let _ = deployment::Entity::update_many()
                            .col_expr(
                                deployment::Column::BuildStatus,
                                sea_orm::ActiveEnum::as_enum(&DeploymentBuildStatus::Pending),
                            )
                            .col_expr(
                                deployment::Column::BuildNodeId,
                                sea_orm::sea_query::Expr::value(Option::<Uuid>::None),
                            )
                            .col_expr(
                                deployment::Column::ClaimedAt,
                                sea_orm::sea_query::Expr::value(
                                    Option::<time::OffsetDateTime>::None,
                                ),
                            )
                            .filter(deployment::Column::Id.eq(candidate.id))
                            .filter(deployment::Column::BuildNodeId.eq(node.id))
                            .filter(
                                deployment::Column::BuildStatus.eq(DeploymentBuildStatus::Claimed),
                            )
                            .exec(db)
                            .await;
                        quota.release_build_slot(team.id).await;
                        return Err(AppError::Infrastructure {
                            op: OP,
                            source: anyhow::Error::new(error),
                        });
                    }
                }
            }
            None => None,
        };

        // The claim is committed; a missing timeline event must not fail it.
        if let Err(error) = deployments::append_event(
            db,
            candidate.id,
            crate::infra::database::entity::DeploymentEventKind::Build,
            "build status changed to claimed",
            json!({ "status": "claimed", "build_node_id": node.id }),
        )
        .await
        {
            tracing::warn!(
                operation = OP,
                %error,
                "failed to append claim event"
            );
        }

        let root_directory = candidate
            .source_metadata
            .get("root_directory")
            .and_then(|value| value.as_str())
            .map(str::to_owned);

        return Ok(ok_response(ClaimResponse {
            deployment: Some(ClaimedDeployment {
                deployment_id: candidate.id,
                project_id: candidate.project_id,
                team_id: candidate.team_id,
                environment: deployments::environment_value(&candidate.environment).to_owned(),
                runtime_kind: crate::domain::projects::runtime_value(&candidate.runtime_kind)
                    .to_owned(),
                repository_url: candidate.source_repository_url.clone().unwrap_or_default(),
                branch: candidate.source_branch.clone(),
                commit_hash: candidate.commit_hash.clone(),
                root_directory,
                install_command: candidate.install_command.clone(),
                build_command: candidate.build_command.clone(),
                output_directory: candidate.output_directory.clone(),
                build_timeout_seconds,
                preview_host: candidate.preview_host.clone(),
                source_credential_lease,
            }),
        }));
    }

    Ok(ok_response(ClaimResponse { deployment: None }))
}

/// POST /api/v1/internal/deployments/{deployment_id}/source-credential
pub async fn redeem_source_credential(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
    Json(body): Json<RedeemGitCredentialRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.deployments.source_credential";
    let db = super::database(&state, OP)?;
    build_owned_deployment(db, &node, deployment_id, OP).await?;
    let keyring = state.config.read().unwrap().secrets.git_credentials.clone();
    let redeemed =
        source_credentials::redeem_lease(db, &keyring, node.id, deployment_id, &body.lease)
            .await
            .map_err(|error| match error {
                source_credentials::SourceCredentialError::InvalidLease => AppError::Unauthorized {
                    op: OP,
                    message: "source credential lease is invalid or expired".to_owned(),
                },
                source_credentials::SourceCredentialError::Revoked => AppError::Conflict {
                    op: OP,
                    message: "source credential has been revoked".to_owned(),
                },
                source_credentials::SourceCredentialError::Database(source) => {
                    AppError::Infrastructure {
                        op: OP,
                        source: source.into(),
                    }
                }
                source_credentials::SourceCredentialError::Other(source) => {
                    AppError::Infrastructure { op: OP, source }
                }
                _ => AppError::Internal {
                    op: OP,
                    message: "source credential could not be decrypted".to_owned(),
                },
            })?;
    Ok(ok_response(RedeemGitCredentialResponse {
        credential: redeemed.credential,
        host: redeemed.host,
        port: redeemed.port,
    }))
}

/// POST /api/v1/internal/deployments/{deployment_id}/ssh-host-key
pub async fn observe_ssh_host_key(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
    Json(body): Json<ObserveSshHostKeyRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.deployments.ssh_host_key";
    let db = super::database(&state, OP)?;
    let deployment = build_owned_deployment(db, &node, deployment_id, OP).await?;
    let endpoint = deployment
        .source_repository_url
        .as_deref()
        .and_then(|url| grass_git_source::parse_repository_url(url).ok())
        .filter(|endpoint| endpoint.transport == grass_git_source::GitTransport::Ssh)
        .ok_or_else(|| AppError::Validation {
            op: OP,
            message: "deployment does not use an SSH repository".to_owned(),
        })?;
    if !endpoint.host.eq_ignore_ascii_case(&body.host) || endpoint.port != body.port {
        return Err(AppError::Validation {
            op: OP,
            message: "SSH host key endpoint does not match deployment".to_owned(),
        });
    }
    let key = ssh_host_keys::observe(
        db,
        ssh_host_keys::ObserveHostKeyParams {
            team_id: deployment.team_id,
            host: endpoint.host,
            port: endpoint.port,
            key_type: body.key_type,
            public_key: body.public_key,
            fingerprint_sha256: body.fingerprint_sha256,
            node_id: node.id,
        },
    )
    .await
    .map_err(|error| match error {
        ssh_host_keys::SshHostKeyError::Invalid => AppError::Validation {
            op: OP,
            message: "SSH host key payload is invalid".to_owned(),
        },
        ssh_host_keys::SshHostKeyError::NotFound => AppError::NotFound {
            op: OP,
            message: "SSH host key not found".to_owned(),
        },
        ssh_host_keys::SshHostKeyError::Database(source) => AppError::Infrastructure {
            op: OP,
            source: source.into(),
        },
    })?;
    let approved = key.status == crate::infra::database::entity::SshHostKeyStatus::Approved;
    Ok(ok_response(ObserveSshHostKeyResponse {
        approved,
        known_hosts_line: approved.then(|| ssh_host_keys::known_hosts_line(&key)),
    }))
}

// --- Stage reports ----------------------------------------------------------

/// POST /api/v1/internal/deployments/{deployment_id}/stage
pub async fn stage(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
    Json(body): Json<StageRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.deployments.stage";
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;
    let deployment = build_owned_deployment(db, &node, deployment_id, OP).await?;
    let quota = QuotaService::new(db, cache);

    // A user cancel wins over any progress report: tell the Node to stop.
    let cancel_flagged = cache
        .get(&cancel_flag_key(deployment_id))
        .await
        .ok()
        .flatten()
        .is_some();
    if crate::features::api::v1::projects::deployments::cancellation_was_requested(
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
            let (updated, build_transitioned, ready_action) =
                if matches!(target, DeploymentBuildStatus::Ready) {
                    finalize_ready(db, deployment, transition).await?
                } else if matches!(
                    target,
                    DeploymentBuildStatus::Failed | DeploymentBuildStatus::Canceled
                ) {
                    let transaction =
                        db.begin()
                            .await
                            .map_err(|source| AppError::Infrastructure {
                                op: OP,
                                source: source.into(),
                            })?;
                    let updated = delivery::transition_unsuccessful_build(
                        &transaction,
                        deployment,
                        transition,
                    )
                    .await
                    .map_err(|error| {
                        crate::features::api::v1::projects::deployments::map_delivery_error(
                            error, OP,
                        )
                    })?;
                    transaction
                        .commit()
                        .await
                        .map_err(|source| AppError::Infrastructure {
                            op: OP,
                            source: source.into(),
                        })?;
                    (updated, true, ReadyReleaseAction::None)
                } else {
                    let updated = deployments::transition_build(db, deployment, transition)
                        .await
                        .map_err(|error| {
                            crate::features::api::v1::projects::deployments::map_state_error(
                                error, OP,
                            )
                        })?;
                    (updated, true, ReadyReleaseAction::None)
                };

            if was_started {
                let _ = audits::create_audit_event(
                    db,
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
                .await;
            }

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

                let _ = audits::create_audit_event(
                    db,
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
                .await;

                // Record the persisted build log as an artifact row once.
                if super::storage(&state)
                    .read_build_log(updated.project_id, updated.id)
                    .await
                    .ok()
                    .flatten()
                    .is_some()
                {
                    let _ = record_build_log_artifact(db, &updated).await;
                }
            }

            if matches!(ready_action, ReadyReleaseAction::RequestReview) {
                let _ = audits::create_audit_event(
                    db,
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
                .await;
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
        .one(db)
        .await?;
    if existing.is_some() {
        return Ok(());
    }

    deployment_artifact::ActiveModel {
        id: Set(Uuid::now_v7()),
        deployment_id: Set(deployment.id),
        kind: Set(DeploymentArtifactKind::BuildLog),
        storage_path: Set(LocalStorage::build_log_relative_path(
            deployment.project_id,
            deployment.id,
        )),
        checksum_sha256: Set(None),
        size_bytes: Set(None),
        manifest: Set(json!({})),
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
    db: &sea_orm::DatabaseConnection,
    requested_deployment: deployment::Model,
    transition: BuildTransition,
) -> Result<(deployment::Model, bool, ReadyReleaseAction), AppError> {
    const OP: &str = "internal.deployments.finalize_ready";
    let transaction = db
        .begin()
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

    // Serialize retries for this deployment and make the decision from the
    // latest row, not from the pre-transaction request snapshot.
    let deployment = deployments::get_by_id_for_update(&transaction, requested_deployment.id)
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
        deployments::transition_build(&transaction, deployment, transition)
            .await
            .map_err(|error| {
                crate::features::api::v1::projects::deployments::map_state_error(error, OP)
            })?
    } else {
        deployment
    };
    let policy = deployments::review_policy(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let action = deployments::ready_release_action(
        policy.mode_for(&deployment.environment),
        &deployment.release_status,
    );
    let deployment = match action {
        ReadyReleaseAction::Activate => {
            deployments::activate(&transaction, deployment, ReleaseReason::Auto, None)
                .await
                .map_err(|error| {
                    crate::features::api::v1::projects::deployments::map_state_error(error, OP)
                })?
        }
        ReadyReleaseAction::RequestReview => {
            deployments::request_review(&transaction, deployment, None)
                .await
                .map_err(|error| {
                    crate::features::api::v1::projects::deployments::map_state_error(error, OP)
                })?
                .0
        }
        ReadyReleaseAction::None => deployment,
    };
    delivery::reconcile_environment(
        &transaction,
        deployment.project_id,
        deployment.environment.clone(),
    )
    .await
    .map_err(|source| AppError::Infrastructure {
        op: OP,
        source: source.into(),
    })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    if matches!(action, ReadyReleaseAction::Activate) {
        tracing::info!(
            operation = OP,
            deployment_id = %deployment.id,
            environment = deployments::environment_value(&deployment.environment),
            "deployment auto-activated"
        );
    }
    Ok((deployment, build_transitioned, action))
}

pub(super) async fn auto_activate_if_allowed(
    transaction: &sea_orm::DatabaseTransaction,
    deployment: deployment::Model,
) -> Result<(), AppError> {
    const OP: &str = "internal.deployments.auto_activate";
    let project_id = deployment.project_id;
    let environment = deployment.environment.clone();
    let activated = if deployment.pending_release_reason.is_some() {
        delivery::complete_pending_release(transaction, deployment)
            .await
            .map_err(|error| {
                crate::features::api::v1::projects::deployments::map_delivery_error(error, OP)
            })?
    } else {
        let policy = deployments::review_policy(transaction)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        let action = deployments::serve_ready_release_action(
            policy.mode_for(&deployment.environment),
            &deployment.release_status,
        );
        if matches!(action, ReadyReleaseAction::Activate) {
            Some(
                deployments::activate(transaction, deployment, ReleaseReason::Auto, None)
                    .await
                    .map_err(|error| {
                        crate::features::api::v1::projects::deployments::map_state_error(error, OP)
                    })?,
            )
        } else {
            None
        }
    };
    delivery::reconcile_environment(transaction, project_id, environment)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    if let Some(deployment) = activated {
        tracing::info!(
            operation = OP,
            deployment_id = %deployment.id,
            environment = deployments::environment_value(&deployment.environment),
            "deployment auto-activated after Serve became ready"
        );
    }
    Ok(())
}

// --- Build log --------------------------------------------------------------

pub(crate) fn log_seq_key(deployment_id: Uuid) -> String {
    format!("deployment:{deployment_id}:log_seq")
}

/// PUT /api/v1/internal/deployments/{deployment_id}/build-log
pub async fn append_build_log(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
    Json(body): Json<AppendBuildLogRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.deployments.build_log";
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;
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

    super::storage(&state)
        .append_build_log(deployment.project_id, deployment.id, &buffer)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    let _ = cache
        .set(
            &log_seq_key(deployment.id),
            &last_seq.to_string(),
            std::time::Duration::from_secs(60 * 60 * 24),
        )
        .await;

    Ok(ok_response(AppendBuildLogResponse { last_seq }))
}

// --- Artifact upload / download ---------------------------------------------

const BYTES_PER_MB: u64 = 1024 * 1024;

#[derive(Debug)]
struct ArtifactUploadMetadata {
    checksum_sha256: String,
    packed_size_bytes: u64,
    unpacked_size_bytes: u64,
    disk_mb: i64,
}

fn parse_upload_metadata(headers: &HeaderMap) -> Result<ArtifactUploadMetadata, String> {
    let required = |name: &'static str| {
        headers
            .get(name)
            .ok_or_else(|| format!("missing {name} header"))?
            .to_str()
            .map(str::to_owned)
            .map_err(|_| format!("invalid {name} header"))
    };
    let checksum_sha256 = required(artifact_headers::CHECKSUM_SHA256)?;
    if checksum_sha256.len() != 64
        || !checksum_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(format!(
            "invalid {} header",
            artifact_headers::CHECKSUM_SHA256
        ));
    }
    let parse_size = |name: &'static str| -> Result<u64, String> {
        let value = required(name)?;
        let value = value
            .parse::<u64>()
            .map_err(|_| format!("invalid {name} header"))?;
        if value == 0 {
            return Err(format!("invalid {name} header"));
        }
        Ok(value)
    };
    let packed_size_bytes = parse_size(artifact_headers::PACKED_SIZE_BYTES)?;
    let unpacked_size_bytes = parse_size(artifact_headers::UNPACKED_SIZE_BYTES)?;
    let disk_mb = i64::try_from(unpacked_size_bytes.div_ceil(BYTES_PER_MB))
        .map_err(|_| "unpacked artifact size exceeds the supported range".to_owned())?;
    Ok(ArtifactUploadMetadata {
        checksum_sha256,
        packed_size_bytes,
        unpacked_size_bytes,
        disk_mb,
    })
}

fn validate_serve_disk(
    capacity_disk_mb: i64,
    current_deployment_disk_mb: i64,
    usage: NodeUsage,
    actual_disk_mb: i64,
) -> Result<(), String> {
    let current = u64::try_from(current_deployment_disk_mb)
        .map_err(|_| "current deployment disk usage is invalid".to_owned())?;
    let actual =
        u64::try_from(actual_disk_mb).map_err(|_| "artifact disk usage is invalid".to_owned())?;
    let capacity = u64::try_from(capacity_disk_mb)
        .map_err(|_| "assigned Serve Node disk capacity is invalid".to_owned())?;
    let required = usage.disk_mb.saturating_sub(current).saturating_add(actual);
    if required > capacity {
        return Err(format!(
            "artifact needs {required} MB but the assigned Serve Node has {capacity} MB"
        ));
    }
    Ok(())
}

fn optional_header(headers: &HeaderMap, name: &'static str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

/// PUT /api/v1/internal/deployments/{deployment_id}/static-site
pub async fn upload_static_site(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
    headers: HeaderMap,
    body: Body,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.deployments.static_site";
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;
    let deployment = build_owned_deployment(db, &node, deployment_id, OP).await?;
    let upload = parse_upload_metadata(&headers)
        .map_err(|message| AppError::Validation { op: OP, message })?;
    let team = team_for(db, deployment.team_id, OP).await?;
    let quota = QuotaService::new(db, cache);
    let artifact_max_mb = quota
        .scalar_limit(OP, &team, QuotaDimension::ArtifactMaxMb)
        .await?;
    let max_bytes = match artifact_max_mb {
        Some(max_mb) => u64::try_from(max_mb)
            .ok()
            .and_then(|max_mb| max_mb.checked_mul(BYTES_PER_MB))
            .ok_or_else(|| AppError::Validation {
                op: OP,
                message: "artifact quota is invalid".to_owned(),
            })?,
        None => u64::MAX,
    };
    if upload.packed_size_bytes > max_bytes {
        return Err(crate::infra::quota::quota_exceeded_error(
            OP,
            QuotaDimension::ArtifactMaxMb,
        ));
    }
    let pending = match super::storage(&state)
        .write_artifact_stream(
            deployment.project_id,
            deployment.id,
            body.into_data_stream(),
            max_bytes,
        )
        .await
    {
        Ok(pending) => pending,
        Err(StorageError::LimitExceeded { .. }) => {
            return Err(crate::infra::quota::quota_exceeded_error(
                OP,
                QuotaDimension::ArtifactMaxMb,
            ));
        }
        Err(source) => {
            return Err(AppError::Infrastructure {
                op: OP,
                source: source.into(),
            });
        }
    };
    if pending.size_bytes == 0 {
        pending.discard().await;
        return Err(AppError::Validation {
            op: OP,
            message: "artifact body is empty".to_owned(),
        });
    }
    if u64::try_from(pending.size_bytes).ok() != Some(upload.packed_size_bytes) {
        pending.discard().await;
        return Err(AppError::Validation {
            op: OP,
            message: "artifact packed size does not match its metadata".to_owned(),
        });
    }
    if pending.checksum_sha256 != upload.checksum_sha256 {
        pending.discard().await;
        return Err(AppError::Validation {
            op: OP,
            message: "artifact checksum does not match its metadata".to_owned(),
        });
    }
    let size_mb = i64::try_from(upload.packed_size_bytes.div_ceil(BYTES_PER_MB)).map_err(|_| {
        AppError::Validation {
            op: OP,
            message: "artifact packed size exceeds the supported range".to_owned(),
        }
    })?;
    let reservation = match quota
        .reserve(
            OP,
            &team,
            None,
            &[QuotaCharge::amount(QuotaDimension::StorageMb, size_mb)],
        )
        .await
    {
        Ok(reservation) => reservation,
        Err(error) => {
            pending.discard().await;
            return Err(error);
        }
    };
    let manifest = json!({
        "runtime_kind": optional_header(&headers, artifact_headers::RUNTIME_KIND),
        "output_api_version": optional_header(&headers, artifact_headers::OUTPUT_API_VERSION),
        "framework_name": optional_header(&headers, artifact_headers::FRAMEWORK_NAME),
        "framework_version": optional_header(&headers, artifact_headers::FRAMEWORK_VERSION),
        "packed_size_bytes": upload.packed_size_bytes,
        "unpacked_size_bytes": upload.unpacked_size_bytes,
    });
    let team_id = deployment.team_id;
    let project_id = deployment.project_id;
    let result: Result<_, AppError> = async move {
        let transaction = db
            .begin()
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
        let current = deployments::get_by_id(&transaction, deployment_id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?
            .ok_or_else(|| AppError::NotFound {
                op: OP,
                message: "deployment not found".to_owned(),
            })?;
        if current.build_node_id != Some(node.id) {
            return Err(AppError::Forbidden {
                op: OP,
                message: "deployment build is not assigned to this node".to_owned(),
            });
        }
        let serve_node_id = current.serve_node_id.ok_or_else(|| AppError::Validation {
            op: OP,
            message: "deployment is not assigned to a Serve Node".to_owned(),
        })?;
        let serve_node = node::Entity::find_by_id(serve_node_id)
            .filter(node::Column::DeletedAt.is_null())
            .one(&transaction)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .ok_or_else(|| AppError::Validation {
                op: OP,
                message: "assigned Serve Node does not exist".to_owned(),
            })?;
        if !serve_node.serve_enabled {
            return Err(AppError::Validation {
                op: OP,
                message: "assigned node does not have Serve capability".to_owned(),
            });
        }
        let usage = scheduler::node_usage(&transaction)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .remove(&serve_node_id)
            .unwrap_or_default();
        validate_serve_disk(
            serve_node.capacity_disk_mb,
            current.serve_disk_mb,
            usage,
            upload.disk_mb,
        )
        .map_err(|message| AppError::Validation { op: OP, message })?;

        let mut active: deployment::ActiveModel = current.into();
        active.serve_disk_mb = Set(upload.disk_mb);
        active
            .update(&transaction)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;

        let stored = pending
            .finalize()
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
        let existing = deployment_artifact::Entity::find()
            .filter(deployment_artifact::Column::DeploymentId.eq(deployment_id))
            .filter(deployment_artifact::Column::Kind.eq(DeploymentArtifactKind::GrassOutput))
            .one(&transaction)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
        let artifact = if let Some(existing) = existing {
            let mut active: deployment_artifact::ActiveModel = existing.into();
            active.storage_path = Set(stored.relative_path.clone());
            active.checksum_sha256 = Set(Some(stored.checksum_sha256.clone()));
            active.size_bytes = Set(Some(stored.size_bytes));
            active.manifest = Set(manifest);
            active.update(&transaction).await
        } else {
            deployment_artifact::ActiveModel {
                id: Set(Uuid::now_v7()),
                deployment_id: Set(deployment_id),
                kind: Set(DeploymentArtifactKind::GrassOutput),
                storage_path: Set(stored.relative_path.clone()),
                checksum_sha256: Set(Some(stored.checksum_sha256.clone())),
                size_bytes: Set(Some(stored.size_bytes)),
                manifest: Set(manifest),
                created_at: Set(time::OffsetDateTime::now_utc()),
            }
            .insert(&transaction)
            .await
        }
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
        transaction
            .commit()
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
        Ok((artifact, stored))
    }
    .await;
    let (artifact, stored) = match result {
        Ok(result) => result,
        Err(error) => {
            quota.rollback(reservation).await;
            return Err(error);
        }
    };

    quota
        .commit(OP, reservation, "deployment_artifact", Some(artifact.id))
        .await?;

    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: None,
            actor_node_id: Some(node.id),
            team_id: Some(team_id),
            action: "artifact.uploaded".to_owned(),
            target_type: "deployment".to_owned(),
            target_id: Some(deployment_id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({
                "size_bytes": stored.size_bytes,
                "unpacked_size_bytes": upload.unpacked_size_bytes,
                "checksum_sha256": stored.checksum_sha256,
                "project_id": project_id,
            }),
        },
    )
    .await;

    Ok(ok_response(UploadArtifactResponse {
        artifact_id: artifact.id,
        size_bytes: stored.size_bytes,
        checksum_sha256: stored.checksum_sha256,
    }))
}

/// GET /api/v1/internal/deployments/{deployment_id}/artifact
///
/// Lets a serve Node re-fetch the grass-output archive after cache loss.
pub async fn download_artifact(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
) -> Result<Response, AppError> {
    const OP: &str = "internal.deployments.download_artifact";
    let db = super::database(&state, OP)?;

    let deployment = deployments::get_by_id(db, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment not found".to_owned(),
        })?;
    if deployment.serve_node_id != Some(node.id) {
        return Err(AppError::Forbidden {
            op: OP,
            message: "deployment is not assigned to this Serve Node".to_owned(),
        });
    }
    let artifact = deployment_artifact::Entity::find()
        .filter(deployment_artifact::Column::DeploymentId.eq(deployment.id))
        .filter(deployment_artifact::Column::Kind.eq(DeploymentArtifactKind::GrassOutput))
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "artifact not found".to_owned(),
        })?;
    let opened = super::storage(&state)
        .open_artifact(&artifact.storage_path)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "artifact not found".to_owned(),
        })?;
    let packed_size_bytes = artifact
        .size_bytes
        .and_then(|size| u64::try_from(size).ok())
        .ok_or_else(|| AppError::Internal {
            op: OP,
            message: "artifact packed size metadata is missing".to_owned(),
        })?;
    if opened.size_bytes != packed_size_bytes {
        return Err(AppError::Internal {
            op: OP,
            message: "artifact file size does not match its metadata".to_owned(),
        });
    }
    let checksum_sha256 = artifact.checksum_sha256.ok_or_else(|| AppError::Internal {
        op: OP,
        message: "artifact checksum metadata is missing".to_owned(),
    })?;
    let unpacked_size_bytes = deployments::artifact_unpacked_size_bytes(&artifact.manifest)
        .ok_or_else(|| AppError::Internal {
            op: OP,
            message: "artifact unpacked size metadata is invalid".to_owned(),
        })?;
    let mut response = Response::new(Body::from_stream(ReaderStream::new(opened.file)));
    let response_headers = response.headers_mut();
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    for (name, value) in [
        (
            header::CONTENT_LENGTH.as_str(),
            packed_size_bytes.to_string(),
        ),
        (
            artifact_headers::PACKED_SIZE_BYTES,
            packed_size_bytes.to_string(),
        ),
        (
            artifact_headers::UNPACKED_SIZE_BYTES,
            unpacked_size_bytes.to_string(),
        ),
        (artifact_headers::CHECKSUM_SHA256, checksum_sha256),
    ] {
        response_headers.insert(
            axum::http::HeaderName::from_static(name),
            HeaderValue::from_str(&value).map_err(|_| AppError::Internal {
                op: OP,
                message: "artifact response metadata is invalid".to_owned(),
            })?,
        );
    }
    Ok(response)
}

#[cfg(test)]
mod artifact_tests {
    use super::*;

    #[test]
    fn upload_metadata_requires_sizes_and_checksum() {
        let mut headers = HeaderMap::new();
        headers.insert(
            artifact_headers::CHECKSUM_SHA256,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .parse()
                .unwrap(),
        );
        headers.insert(artifact_headers::PACKED_SIZE_BYTES, "9".parse().unwrap());
        headers.insert(
            artifact_headers::UNPACKED_SIZE_BYTES,
            "1025".parse().unwrap(),
        );

        let metadata = parse_upload_metadata(&headers).unwrap();

        assert_eq!(metadata.packed_size_bytes, 9);
        assert_eq!(metadata.unpacked_size_bytes, 1025);
        assert_eq!(metadata.disk_mb, 1);

        headers.remove(artifact_headers::UNPACKED_SIZE_BYTES);
        assert_eq!(
            parse_upload_metadata(&headers).unwrap_err(),
            "missing x-grass-unpacked-size-bytes header"
        );
    }

    #[test]
    fn actual_disk_replaces_the_deployments_reserved_disk() {
        let usage = NodeUsage {
            cpu_millicores: 400,
            memory_mb: 512,
            disk_mb: 900,
            deployments: 3,
        };

        assert!(validate_serve_disk(1_024, 512, usage, 600).is_ok());
        assert_eq!(
            validate_serve_disk(1_024, 512, usage, 700).unwrap_err(),
            "artifact needs 1088 MB but the assigned Serve Node has 1024 MB"
        );
    }
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
}
