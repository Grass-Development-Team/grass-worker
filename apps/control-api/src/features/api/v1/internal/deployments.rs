use axum::{
    Extension, Json,
    body::Bytes,
    extract::{Path, State},
    http::HeaderMap,
    response::IntoResponse,
};
use grass_cache::Cache;
use grass_node_protocol::{
    AppendBuildLogRequest, AppendBuildLogResponse, ClaimRequest, ClaimResponse, ClaimedDeployment,
    ReportedStatus, StageRequest, StageResponse, UploadArtifactResponse, artifact_headers,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, TransactionTrait};
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        deployments::{self, BuildTransition, ReviewMode},
        quotas::QuotaDimension,
        teams,
    },
    infra::{
        database::entity::{
            AuditEventResult, DeploymentArtifactKind, DeploymentBuildStatus,
            DeploymentReleaseStatus, ProjectRuntime, ReleaseReason, deployment,
            deployment_artifact, node, team,
        },
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
        quota::{QuotaCharge, QuotaService},
        storage::LocalStorage,
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

async fn owned_deployment(
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
    if deployment.node_id != Some(node.id) {
        return Err(AppError::Forbidden {
            op,
            message: "deployment is not assigned to this node".to_owned(),
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

    if body.capacity == 0 {
        return Ok(ok_response(ClaimResponse { deployment: None }));
    }

    // Oldest pending static deployments first. Non-static runtimes never
    // reach the queue; they fail at creation time.
    let candidates = deployment::Entity::find()
        .filter(deployment::Column::BuildStatus.eq(DeploymentBuildStatus::Pending))
        .filter(deployment::Column::RuntimeKind.eq(ProjectRuntime::Static))
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
        // run; released on any failure below.
        if !quota.acquire_build_slot(OP, &team).await? {
            continue;
        }

        // Optimistic claim: only one node can flip pending → claimed.
        let claim_result = deployment::Entity::update_many()
            .col_expr(
                deployment::Column::BuildStatus,
                sea_orm::sea_query::Expr::value(DeploymentBuildStatus::Claimed),
            )
            .col_expr(
                deployment::Column::NodeId,
                sea_orm::sea_query::Expr::value(node.id),
            )
            .col_expr(
                deployment::Column::ClaimedAt,
                sea_orm::sea_query::Expr::value(time::OffsetDateTime::now_utc()),
            )
            .filter(deployment::Column::Id.eq(candidate.id))
            .filter(deployment::Column::BuildStatus.eq(DeploymentBuildStatus::Pending))
            .exec(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;

        if claim_result.rows_affected == 0 {
            quota.release_build_slot(team.id).await;
            continue;
        }

        deployments::append_event(
            db,
            candidate.id,
            crate::infra::database::entity::DeploymentEventKind::Build,
            "build status changed to claimed",
            json!({ "status": "claimed", "node_id": node.id }),
        )
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

        let build_timeout_seconds = quota
            .scalar_limit(OP, &team, QuotaDimension::BuildTimeoutSeconds)
            .await?;

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
            }),
        }));
    }

    Ok(ok_response(ClaimResponse { deployment: None }))
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
    let deployment = owned_deployment(db, &node, deployment_id, OP).await?;
    let quota = QuotaService::new(db, cache);

    // A user cancel wins over any progress report: tell the Node to stop.
    let cancel_flagged = cache
        .get(&cancel_flag_key(deployment_id))
        .await
        .ok()
        .flatten()
        .is_some();
    if matches!(deployment.build_status, DeploymentBuildStatus::Canceled) || cancel_flagged {
        // The Node acknowledges by reporting canceled; account build minutes
        // when it does.
        if matches!(body.status, Some(ReportedStatus::Canceled)) {
            let _ = cache.delete(&cancel_flag_key(deployment_id)).await;
            quota.release_build_slot(deployment.team_id).await;
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
            let updated = deployments::transition_build(
                db,
                deployment,
                BuildTransition {
                    to: target.clone(),
                    stage: body.stage.clone(),
                    failure_code: body.failure_code.clone(),
                    failure_message: body.failure_message.clone(),
                    node_id: Some(node.id),
                },
            )
            .await
            .map_err(|error| {
                crate::features::api::v1::projects::deployments::map_state_error(error, OP)
            })?;

            if was_started {
                let _ = audits::create_audit_event(
                    db,
                    CreateAuditEventParams {
                        actor_user_id: None,
                        team_id: Some(team_id),
                        action: "deployment.build_started".to_owned(),
                        target_type: "deployment".to_owned(),
                        target_id: Some(updated.id),
                        result: AuditEventResult::Success,
                        reason: None,
                        metadata: json!({ "node_id": node.id }),
                    },
                )
                .await;
            }

            if is_terminal {
                quota.release_build_slot(team_id).await;
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

                if matches!(target, DeploymentBuildStatus::Ready) {
                    auto_activate_if_allowed(db, updated).await?;
                }
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

/// Auto-activation after a successful build, controlled by the release
/// review policy: `auto` environments activate immediately, `manual`
/// environments wait for review and promote.
async fn auto_activate_if_allowed(
    db: &sea_orm::DatabaseConnection,
    deployment: deployment::Model,
) -> Result<(), AppError> {
    const OP: &str = "internal.deployments.auto_activate";
    let policy = deployments::review_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    if !matches!(policy.mode_for(&deployment.environment), ReviewMode::Auto) {
        return Ok(());
    }
    if !matches!(deployment.release_status, DeploymentReleaseStatus::Draft) {
        return Ok(());
    }

    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let activated = deployments::activate(&transaction, deployment, ReleaseReason::Auto, None)
        .await
        .map_err(|error| {
            crate::features::api::v1::projects::deployments::map_state_error(error, OP)
        })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    tracing::info!(
        operation = OP,
        deployment_id = %activated.id,
        environment = deployments::environment_value(&activated.environment),
        "deployment auto-activated"
    );
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
    let deployment = owned_deployment(db, &node, deployment_id, OP).await?;

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

/// PUT /api/v1/internal/deployments/{deployment_id}/static-site
pub async fn upload_static_site(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.deployments.static_site";
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;
    let deployment = owned_deployment(db, &node, deployment_id, OP).await?;

    if bytes.is_empty() {
        return Err(AppError::Validation {
            op: OP,
            message: "artifact body is empty".to_owned(),
        });
    }

    let team = team_for(db, deployment.team_id, OP).await?;
    let quota = QuotaService::new(db, cache);
    let size_mb = bytes.len().div_ceil(1024 * 1024) as i64;

    if let Some(max_mb) = quota
        .scalar_limit(OP, &team, QuotaDimension::ArtifactMaxMb)
        .await?
        && size_mb > max_mb
    {
        return Err(crate::infra::quota::quota_exceeded_error(
            OP,
            QuotaDimension::ArtifactMaxMb,
        ));
    }

    let reservation = quota
        .reserve(
            OP,
            &team,
            None,
            &[QuotaCharge::amount(QuotaDimension::StorageMb, size_mb)],
        )
        .await?;

    let stored = match super::storage(&state)
        .save_artifact(deployment.project_id, deployment.id, &bytes)
        .await
    {
        Ok(stored) => stored,
        Err(source) => {
            quota.rollback(reservation).await;
            return Err(AppError::Infrastructure { op: OP, source });
        }
    };

    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let manifest = json!({
        "runtime_kind": header(artifact_headers::RUNTIME_KIND),
        "output_api_version": header(artifact_headers::OUTPUT_API_VERSION),
        "framework_name": header(artifact_headers::FRAMEWORK_NAME),
        "framework_version": header(artifact_headers::FRAMEWORK_VERSION),
    });

    use sea_orm::{ActiveModelTrait, ActiveValue::Set};
    let artifact = match (deployment_artifact::ActiveModel {
        id: Set(Uuid::now_v7()),
        deployment_id: Set(deployment.id),
        kind: Set(DeploymentArtifactKind::GrassOutput),
        storage_path: Set(stored.relative_path.clone()),
        checksum_sha256: Set(Some(stored.checksum_sha256.clone())),
        size_bytes: Set(Some(stored.size_bytes)),
        manifest: Set(manifest),
        created_at: Set(time::OffsetDateTime::now_utc()),
    }
    .insert(db)
    .await)
    {
        Ok(artifact) => artifact,
        Err(source) => {
            quota.rollback(reservation).await;
            return Err(AppError::Infrastructure {
                op: OP,
                source: source.into(),
            });
        }
    };

    quota
        .commit(OP, reservation, "deployment_artifact", Some(artifact.id))
        .await?;

    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: None,
            team_id: Some(deployment.team_id),
            action: "artifact.uploaded".to_owned(),
            target_type: "deployment".to_owned(),
            target_id: Some(deployment.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({
                "size_bytes": stored.size_bytes,
                "checksum_sha256": stored.checksum_sha256,
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
    Extension(AuthenticatedNode(_node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.deployments.download_artifact";
    let db = super::database(&state, OP)?;

    // Any authenticated Node may serve any deployment in the first stage;
    // ownership is not required for downloads.
    let deployment = deployments::get_by_id(db, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment not found".to_owned(),
        })?;

    let bytes = super::storage(&state)
        .read_artifact(deployment.project_id, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "artifact not found".to_owned(),
        })?;

    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/zip")],
        bytes,
    ))
}
