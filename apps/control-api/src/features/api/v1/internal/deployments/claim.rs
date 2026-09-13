use axum::{Extension, Json, extract::State, response::IntoResponse};
use grass_node_protocol::ClaimedDeployment;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{deployments, quotas::QuotaDimension, source_credentials, teams},
    infra::{
        database::entity::{
            DeploymentBuildStatus, NodeStatus, ProjectRuntime, deployment, node, team,
        },
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
        quota::QuotaService,
    },
    state::ControlApiState,
};

// --- Claim ------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ClaimRequest {
    /// How many additional builds the Node can take right now.
    pub capacity: u16,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ClaimResponse {
    #[serde(default)]
    pub deployment: Option<ClaimedDeployment>,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route("/deployments/claim", axum::routing::post(claim))
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

// --- Claim ------------------------------------------------------------------
fn can_claim_new_build(status: &NodeStatus) -> bool {
    matches!(status, NodeStatus::Active)
}

fn current_node_for_claim_query(node_id: Uuid) -> sea_orm::Select<node::Entity> {
    node::Entity::find_by_id(node_id)
        .filter(node::Column::DeletedAt.is_null())
        .filter(node::Column::Status.eq(NodeStatus::Active))
        .filter(node::Column::BuildEnabled.eq(true))
        .lock_exclusive()
}

/// POST /api/v1/internal/deployments/claim
pub async fn claim(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Json(body): Json<ClaimRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.deployments.claim";
    let db = crate::infra::http::database(&state, OP)?;
    let cache = crate::infra::http::cache(&state, OP)?;

    if !node.build_enabled {
        return Err(AppError::Forbidden {
            op: OP,
            message: "node does not have build capability".to_owned(),
        });
    }

    if body.capacity == 0 {
        return Ok(ok_response(ClaimResponse { deployment: None }));
    }
    if !can_claim_new_build(&node.status) {
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
        let transaction = match crate::infra::audit::AuditTransaction::begin(db).await {
            Ok(transaction) => transaction,
            Err(source) => {
                quota.release_build_slot(team.id).await;
                return Err(AppError::Infrastructure {
                    op: OP,
                    source: source.into(),
                });
            }
        };
        let current_node = match current_node_for_claim_query(node.id)
            .one(&transaction)
            .await
        {
            Ok(current_node) => current_node,
            Err(source) => {
                let _ = transaction.rollback().await;
                quota.release_build_slot(team.id).await;
                return Err(AppError::Infrastructure {
                    op: OP,
                    source: source.into(),
                });
            }
        };
        if current_node.is_none() {
            let rollback_result =
                transaction
                    .rollback()
                    .await
                    .map_err(|source| AppError::Infrastructure {
                        op: OP,
                        source: source.into(),
                    });
            quota.release_build_slot(team.id).await;
            rollback_result?;
            return Ok(ok_response(ClaimResponse { deployment: None }));
        }

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
            .exec(&transaction)
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
            let rollback_result =
                transaction
                    .rollback()
                    .await
                    .map_err(|source| AppError::Infrastructure {
                        op: OP,
                        source: source.into(),
                    });
            quota.release_build_slot(team.id).await;
            rollback_result?;
            continue;
        }

        let source_credential_lease = match candidate.source_credential_version_id {
            Some(version_id) => {
                match source_credentials::issue_lease(
                    &transaction,
                    node.id,
                    candidate.id,
                    version_id,
                )
                .await
                {
                    Ok(lease) => Some(lease),
                    Err(error) => {
                        let _ = transaction.rollback().await;
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
            &transaction,
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
        if let Err(source) = transaction.commit().await {
            quota.release_build_slot(team.id).await;
            return Err(AppError::Infrastructure {
                op: OP,
                source: source.into(),
            });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::config::ControlApiConfig;
    use crate::infra::database::entity::DeploymentEnvironment;
    use crate::infra::database::entity::DeploymentReleaseStatus;
    use crate::infra::database::entity::DeploymentServeStatus;
    use crate::infra::database::entity::QuotaPeriod;
    use crate::infra::database::entity::TeamKind;
    use crate::infra::database::entity::quota_limit;
    use crate::infra::database::entity::quota_plan;
    use grass_cache::Cache;
    use grass_cache::CacheBackend;
    use grass_cache::CacheStore;
    use sea_orm::DbBackend;
    use sea_orm::DbErr;
    use sea_orm::MockDatabase;
    use sea_orm::QueryTrait;
    use time::OffsetDateTime;
    #[test]
    fn draining_nodes_finish_existing_builds_without_claiming_new_ones() {
        assert!(can_claim_new_build(&NodeStatus::Active));
        assert!(!can_claim_new_build(&NodeStatus::Draining));
        assert!(!can_claim_new_build(&NodeStatus::Offline));
    }

    #[test]
    fn claim_rechecks_and_locks_the_current_node_row() {
        let sql = current_node_for_claim_query(Uuid::nil())
            .build(DbBackend::Postgres)
            .to_string();
        assert!(sql.contains("FOR UPDATE"));
        assert!(sql.contains("deleted_at"));
        assert!(sql.contains("status"));
        assert!(sql.contains("build_enabled"));
    }

    #[tokio::test]
    async fn claim_releases_the_build_slot_when_the_locked_node_query_fails() {
        let now = OffsetDateTime::now_utc();
        let team_id = Uuid::now_v7();
        let project_id = Uuid::now_v7();
        let node_id = Uuid::now_v7();
        let plan_id = Uuid::now_v7();
        let candidate = deployment::Model {
            id: Uuid::now_v7(),
            project_id,
            team_id,
            region: "default".to_owned(),
            build_node_id: None,
            serve_node_id: None,
            environment: DeploymentEnvironment::Preview,
            runtime_kind: ProjectRuntime::Static,
            build_status: DeploymentBuildStatus::Pending,
            serve_status: DeploymentServeStatus::Pending,
            release_status: DeploymentReleaseStatus::Draft,
            serve_cpu_millicores: 0,
            serve_memory_mb: 0,
            serve_disk_mb: 0,
            overcommitted: false,
            source_repository_url: Some("https://example.test/repository.git".to_owned()),
            source_credential_version_id: None,
            source_branch: Some("main".to_owned()),
            commit_hash: None,
            commit_message: None,
            triggered_by_user_id: None,
            install_command: None,
            build_command: None,
            output_directory: None,
            source_metadata: json!({}),
            preview_host: None,
            build_stage: None,
            failure_code: None,
            failure_message: None,
            serve_failure_code: None,
            serve_failure_message: None,
            pending_release_reason: None,
            pending_release_actor_user_id: None,
            pending_release_audit_visibility: None,
            pending_release_requested_at: None,
            claimed_at: None,
            build_started_at: None,
            build_finished_at: None,
            serve_started_at: None,
            serve_finished_at: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };
        let team = team::Model {
            id: team_id,
            slug: "claim-slot-release".to_owned(),
            name: "Claim Slot Release".to_owned(),
            avatar_version: None,
            kind: TeamKind::Team,
            group_id: None,
            explicit_quota_plan_id: None,
            owner_user_id: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };
        let plan = quota_plan::Model {
            id: plan_id,
            code: "claim-slot-release".to_owned(),
            name: "Claim Slot Release".to_owned(),
            description: None,
            is_default: true,
            enabled: true,
            created_at: now,
            updated_at: now,
        };
        let limits = vec![
            quota_limit::Model {
                id: Uuid::now_v7(),
                quota_plan_id: plan_id,
                dimension: QuotaDimension::ConcurrentBuilds.as_str().to_owned(),
                limit_value: 1,
                period: QuotaPeriod::None,
                created_at: now,
                updated_at: now,
            },
            quota_limit::Model {
                id: Uuid::now_v7(),
                quota_plan_id: plan_id,
                dimension: QuotaDimension::BuildTimeoutSeconds.as_str().to_owned(),
                limit_value: 600,
                period: QuotaPeriod::None,
                created_at: now,
                updated_at: now,
            },
        ];
        let database = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([vec![candidate]])
            .append_query_results([vec![team]])
            .append_query_results([vec![plan.clone()]])
            .append_query_results([limits.clone()])
            .append_query_results([vec![plan]])
            .append_query_results([limits])
            .append_query_errors([DbErr::Custom("locked node query failed".to_owned())])
            .into_connection();
        let cache = CacheStore::connect_cache(CacheBackend::Moka, "")
            .await
            .unwrap();
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        state.database.set(database).unwrap();
        assert!(state.cache.set(cache).is_ok());
        let authenticated_node = node::Model {
            id: node_id,
            name: "build-1".to_owned(),
            region: "default".to_owned(),
            token_hash: "unused".to_owned(),
            status: NodeStatus::Active,
            build_enabled: true,
            serve_enabled: false,
            build_concurrency: 1,
            base_url: None,
            work_root: None,
            capacity_cpu_millicores: 1_000,
            capacity_memory_mb: 1_024,
            capacity_disk_mb: 10_240,
            max_deployments: 1,
            metadata: json!({}),
            last_heartbeat_at: Some(now),
            desired_config: None,
            desired_config_revision: 0,
            effective_config: None,
            effective_config_revision: 0,
            config_sync_status: crate::infra::database::entity::NodeConfigSyncStatus::Applied,
            config_sync_error: None,
            node_token_configured: true,
            config_updated_at: None,
            config_applied_at: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };

        let result = claim(
            State(state.clone()),
            Extension(AuthenticatedNode(authenticated_node)),
            Json(ClaimRequest { capacity: 1 }),
        )
        .await;

        assert!(result.is_err());
        let key = format!("quota:team:{team_id}:concurrent_builds");
        assert_eq!(
            state
                .try_cache()
                .unwrap()
                .get(&key)
                .await
                .unwrap()
                .as_deref(),
            Some("0")
        );
    }

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<ClaimRequest, grass_node_protocol::ClaimRequest>(
            "ClaimRequest",
        );
        crate::test_support::assert_node_contract::<
            ClaimResponse,
            grass_node_protocol::ClaimResponse,
        >("ClaimResponse");
    }
}
