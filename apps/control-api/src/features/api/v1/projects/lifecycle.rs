use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::sea_query::LockType;
use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QuerySelect, TransactionTrait,
};
use serde::Deserialize;
use serde_json::json;
use std::future::Future;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        hosts, notifications, projects,
        quotas::QuotaDimension,
        teams,
    },
    infra::{
        database::entity::{
            AuditEventResult, ProjectRuntime, TeamMemberRole, project, project_host_binding, team,
        },
        error::{AppError, ok_response},
        host_provision::service::{DeprovisionOutcome, HostBindingService},
        http::extractors::Session,
        quota::{QuotaCharge, QuotaService},
    },
    state::ControlApiState,
};

async fn record_lifecycle_audit(
    state: &ControlApiState,
    actor: Uuid,
    team_id: Uuid,
    action: &str,
    project_id: Uuid,
    metadata: serde_json::Value,
) {
    if let Some(db) = state.try_database() {
        let _ = audits::create_audit_event(
            db,
            CreateAuditEventParams {
                actor_user_id: Some(actor),
                actor_node_id: None,
                team_id: Some(team_id),
                action: action.to_owned(),
                target_type: "project".to_owned(),
                target_id: Some(project_id),
                result: AuditEventResult::Success,
                reason: None,
                metadata,
            },
        )
        .await;
    }
}

async fn record_lifecycle_event<C: ConnectionTrait>(
    db: &C,
    actor: Uuid,
    action: &str,
    project: &project::Model,
    target_url: String,
) -> anyhow::Result<()> {
    audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(actor),
            actor_node_id: None,
            team_id: Some(project.team_id),
            action: action.to_owned(),
            target_type: "project".to_owned(),
            target_id: Some(project.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({}),
        },
    )
    .await?;
    notifications::create_project_notification(
        db,
        notifications::CreateProjectNotification {
            project,
            actor_user_id: actor,
            action,
            reason: None,
            target_url,
        },
    )
    .await?;
    Ok(())
}

fn runtime_dimension(runtime: &ProjectRuntime) -> QuotaDimension {
    match runtime {
        ProjectRuntime::Ssr => QuotaDimension::ProjectsSsr,
        _ => QuotaDimension::ProjectsStatic,
    }
}

pub(crate) struct SoftDeleteProjectResult {
    pub(crate) project: project::Model,
    pub(crate) bindings: Vec<project_host_binding::Model>,
    pub(crate) newly_deleted: bool,
}

pub(crate) async fn soft_delete_project_records<C: ConnectionTrait>(
    db: &C,
    project: project::Model,
) -> anyhow::Result<SoftDeleteProjectResult> {
    let project = project::Entity::find()
        .filter(project::Column::Id.eq(project.id))
        .lock(LockType::Update)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("project not found"))?;
    if let Some(deletion_generation) = project.deleted_at {
        let bindings = project_host_binding::Entity::find()
            .filter(project_host_binding::Column::ProjectId.eq(project.id))
            .filter(project_host_binding::Column::DeletedAt.eq(deletion_generation))
            .lock(LockType::Update)
            .all(db)
            .await?;
        return Ok(SoftDeleteProjectResult {
            project,
            bindings,
            newly_deleted: false,
        });
    }

    let bindings = hosts::list_bindings_for_project_for_update(db, project.id).await?;
    let generation = OffsetDateTime::now_utc();
    for binding in &bindings {
        hosts::soft_delete_binding_at(db, binding.clone(), generation).await?;
    }
    let project = projects::soft_delete_at(db, project, generation).await?;
    Ok(SoftDeleteProjectResult {
        project,
        bindings,
        newly_deleted: true,
    })
}

pub(crate) async fn finalize_deleted_project_resources(
    db: &sea_orm::DatabaseConnection,
    cache: &grass_cache::CacheStore,
    op: &'static str,
    project: &project::Model,
    bindings: &[project_host_binding::Model],
) -> Result<Vec<ProjectCleanupWarning>, AppError> {
    finalize_deleted_project_resources_with_cleanup(
        db,
        cache,
        op,
        project,
        bindings,
        |binding| async move {
            if let Some(source_id) = binding.host_source_id
                && let Some(source) =
                    hosts::get_source_by_id_including_deleted(db, source_id).await?
            {
                let outcome = HostBindingService::new(db, cache)
                    .deprovision(op, &binding, &source)
                    .await
                    .map_err(|_| anyhow::anyhow!("host deprovision did not complete"))?;
                if outcome == DeprovisionOutcome::Failed {
                    return Err(anyhow::anyhow!("host deprovision did not complete"));
                }
            }
            Ok(())
        },
    )
    .await
}

pub(crate) async fn release_deleted_project_quota(
    db: &sea_orm::DatabaseConnection,
    cache: &grass_cache::CacheStore,
    op: &'static str,
    project: &project::Model,
    bindings: &[project_host_binding::Model],
) -> Result<(), AppError> {
    let quota = QuotaService::new(db, cache);
    let generation = project.deleted_at.unwrap_or(project.updated_at);
    let host_charge = [QuotaCharge::one(QuotaDimension::Hosts)];

    for binding in bindings {
        quota
            .release_once_for_generation(
                op,
                project.team_id,
                &host_charge,
                "project_host_binding",
                binding.id,
                generation,
            )
            .await?;
    }

    quota
        .release_once_for_generation(
            op,
            project.team_id,
            &[
                QuotaCharge::one(QuotaDimension::Projects),
                QuotaCharge::one(runtime_dimension(&project.runtime)),
            ],
            "project",
            project.id,
            generation,
        )
        .await
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct ProjectCleanupWarning {
    pub code: &'static str,
    pub binding_id: Uuid,
}

pub(crate) async fn finalize_deleted_project_resources_with_cleanup<F, Fut>(
    db: &sea_orm::DatabaseConnection,
    cache: &grass_cache::CacheStore,
    op: &'static str,
    project: &project::Model,
    bindings: &[project_host_binding::Model],
    cleanup: F,
) -> Result<Vec<ProjectCleanupWarning>, AppError>
where
    F: Fn(project_host_binding::Model) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    // Structural quota must be released after tombstones commit but before
    // any provider cleanup can fail. This keeps retries and cache repair safe.
    release_deleted_project_quota(db, cache, op, project, bindings).await?;

    let mut warnings = Vec::new();
    for binding in bindings {
        if cleanup(binding.clone()).await.is_err() {
            tracing::warn!(
                operation = op,
                binding_id = %binding.id,
                "project resource cleanup failed after quota release"
            );
            warnings.push(ProjectCleanupWarning {
                code: "project_resource_cleanup_failed",
                binding_id: binding.id,
            });
        }
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use grass_cache::Cache;

    use super::*;

    #[tokio::test]
    async fn quota_release_runs_when_external_cleanup_returns_a_warning() {
        let project_id = Uuid::now_v7();
        let team_id = Uuid::now_v7();
        let now = time::OffsetDateTime::UNIX_EPOCH;
        let project = project::Model {
            id: project_id,
            team_id,
            created_by_user_id: None,
            slug: "deleted-project".to_owned(),
            name: "Deleted project".to_owned(),
            runtime: ProjectRuntime::Static,
            repository_url: None,
            default_branch: None,
            install_command: None,
            build_command: None,
            output_directory: None,
            source_config: serde_json::json!({}),
            build_config: serde_json::json!({}),
            archived_at: None,
            deleted_at: Some(now),
            created_at: now,
            updated_at: now,
        };

        let count_row = |value: i64| {
            std::collections::BTreeMap::from([(
                "num_items".to_owned(),
                sea_orm::Value::BigInt(Some(value)),
            )])
        };
        let quota_event =
            |dimension: QuotaDimension| crate::infra::database::entity::quota_event::Model {
                id: Uuid::now_v7(),
                team_id,
                dimension: dimension.as_str().to_owned(),
                kind: crate::infra::database::entity::QuotaEventKind::Release,
                delta_value: -1,
                idempotency_key: Some("test-release".to_owned()),
                resource_type: Some("project".to_owned()),
                resource_id: Some(project_id),
                metadata: serde_json::json!({}),
                created_at: now,
            };
        let counter = |dimension: QuotaDimension| {
            crate::infra::database::entity::quota_usage_counter::Model {
                id: Uuid::now_v7(),
                team_id,
                dimension: dimension.as_str().to_owned(),
                used_value: 0,
                period_start: None,
                period_end: None,
                updated_at: now,
            }
        };
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[quota_event(QuotaDimension::Hosts)]])
            .append_query_results([Vec::<
                crate::infra::database::entity::quota_usage_counter::Model,
            >::new()])
            .append_query_results([[counter(QuotaDimension::Hosts)]])
            .append_query_results([[count_row(0)]])
            .append_query_results([[quota_event(QuotaDimension::Projects)]])
            .append_query_results([Vec::<
                crate::infra::database::entity::quota_usage_counter::Model,
            >::new()])
            .append_query_results([[counter(QuotaDimension::Projects)]])
            .append_query_results([[count_row(0)]])
            .append_query_results([[quota_event(QuotaDimension::ProjectsStatic)]])
            .append_query_results([Vec::<
                crate::infra::database::entity::quota_usage_counter::Model,
            >::new()])
            .append_query_results([[counter(QuotaDimension::ProjectsStatic)]])
            .append_query_results([[count_row(0)]])
            .into_connection();
        let cache = grass_cache::CacheStore::Moka(grass_cache::MokaCache::connect());

        let binding = project_host_binding::Model {
            id: Uuid::now_v7(),
            project_id,
            team_id,
            host_source_id: None,
            host: "example.invalid".to_owned(),
            kind: crate::infra::database::entity::HostBindingKind::Custom,
            environment: crate::infra::database::entity::HostBindingEnvironment::Preview,
            status: crate::infra::database::entity::HostBindingStatus::Active,
            failure_reason: None,
            is_primary: true,
            review_status: crate::infra::database::entity::HostReviewStatus::NotRequired,
            reviewed_by_user_id: None,
            reviewed_at: None,
            review_reason: None,
            deleted_at: Some(now),
            created_at: now,
            updated_at: now,
        };
        let result = finalize_deleted_project_resources_with_cleanup(
            &db,
            &cache,
            "test.finalize",
            &project,
            &[binding],
            |_| async { Err(anyhow::anyhow!("provider credential leaked")) },
        )
        .await;

        let warnings = result.expect("cleanup warnings must not prevent quota release");
        assert_eq!(warnings.len(), 1);
        assert!(
            !serde_json::to_string(&warnings)
                .unwrap()
                .contains("provider credential leaked")
        );
        assert_eq!(
            cache
                .get(&format!("quota:team:{team_id}:projects"))
                .await
                .unwrap()
                .as_deref(),
            Some("0")
        );
        assert!(format!("{:?}", db.into_transaction_log()).contains("quota_events"));
    }

    #[tokio::test]
    async fn team_delete_retry_releases_quota_without_repeating_lifecycle_event() {
        let actor_id = Uuid::now_v7();
        let team_id = Uuid::now_v7();
        let tombstone_at = time::OffsetDateTime::from_unix_timestamp(42).unwrap();
        let tombstone = project::Model {
            id: Uuid::now_v7(),
            team_id,
            created_by_user_id: None,
            slug: "deleted-project".to_owned(),
            name: "Deleted project".to_owned(),
            runtime: ProjectRuntime::Static,
            repository_url: None,
            default_branch: None,
            install_command: None,
            build_command: None,
            output_directory: None,
            source_config: serde_json::json!({}),
            build_config: serde_json::json!({}),
            archived_at: None,
            deleted_at: Some(tombstone_at),
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: tombstone_at,
        };
        let project_id = tombstone.id;
        let team = team::Model {
            id: team_id,
            slug: "team".to_owned(),
            name: "Team".to_owned(),
            kind: crate::infra::database::entity::TeamKind::Personal,
            group_id: None,
            explicit_quota_plan_id: None,
            owner_user_id: Some(actor_id),
            deleted_at: None,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
        };
        let membership = crate::infra::database::entity::team_member::Model {
            id: Uuid::now_v7(),
            team_id,
            user_id: actor_id,
            role: TeamMemberRole::Owner,
            invited_by_user_id: None,
            joined_at: time::OffsetDateTime::UNIX_EPOCH,
            deleted_at: None,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
        };
        let binding = project_host_binding::Model {
            id: Uuid::now_v7(),
            project_id,
            team_id,
            host_source_id: None,
            host: "retry.example.invalid".to_owned(),
            kind: crate::infra::database::entity::HostBindingKind::Custom,
            environment: crate::infra::database::entity::HostBindingEnvironment::Preview,
            status: crate::infra::database::entity::HostBindingStatus::Active,
            failure_reason: None,
            is_primary: false,
            review_status: crate::infra::database::entity::HostReviewStatus::NotRequired,
            reviewed_by_user_id: None,
            reviewed_at: None,
            review_reason: None,
            deleted_at: Some(tombstone_at),
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: tombstone_at,
        };
        let count_row = || {
            std::collections::BTreeMap::from([(
                "num_items".to_owned(),
                sea_orm::Value::BigInt(Some(0)),
            )])
        };
        let quota_event = |dimension: &str| crate::infra::database::entity::quota_event::Model {
            id: Uuid::now_v7(),
            team_id,
            dimension: dimension.to_owned(),
            kind: crate::infra::database::entity::QuotaEventKind::Release,
            delta_value: -1,
            idempotency_key: Some("retry-release".to_owned()),
            resource_type: Some("project".to_owned()),
            resource_id: Some(project_id),
            metadata: serde_json::json!({}),
            created_at: tombstone_at,
        };
        let counter =
            |dimension: &str| crate::infra::database::entity::quota_usage_counter::Model {
                id: Uuid::now_v7(),
                team_id,
                dimension: dimension.to_owned(),
                used_value: 0,
                period_start: None,
                period_end: None,
                updated_at: tombstone_at,
            };
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[tombstone.clone()]])
            .append_query_results([[team]])
            .append_query_results([[membership]])
            .append_query_results([[tombstone]])
            .append_query_results([[binding]])
            .append_query_results([[quota_event("hosts")]])
            .append_query_results([Vec::<
                crate::infra::database::entity::quota_usage_counter::Model,
            >::new()])
            .append_query_results([[counter("hosts")]])
            .append_query_results([[count_row()]])
            .append_query_results([[quota_event("projects")]])
            .append_query_results([Vec::<
                crate::infra::database::entity::quota_usage_counter::Model,
            >::new()])
            .append_query_results([[counter("projects")]])
            .append_query_results([[count_row()]])
            .append_query_results([[quota_event("projects.static")]])
            .append_query_results([Vec::<
                crate::infra::database::entity::quota_usage_counter::Model,
            >::new()])
            .append_query_results([[counter("projects.static")]])
            .append_query_results([[count_row()]])
            .into_connection();
        let db_log = db.clone();
        let state = crate::state::ControlApiState::new(
            crate::infra::config::ControlApiConfig::default(),
            "unused.toml",
        );
        state.database.set(db).unwrap();
        assert!(
            state
                .cache
                .set(grass_cache::CacheStore::Moka(
                    grass_cache::MokaCache::connect()
                ))
                .is_ok()
        );
        let session = Session {
            data: grass_session::SessionData {
                user_id: actor_id,
                created_at: time::OffsetDateTime::UNIX_EPOCH,
                last_accessed_at: time::OffsetDateTime::UNIX_EPOCH,
            },
            session_id: "team-session".to_owned(),
        };

        delete(
            axum::extract::State(state),
            session,
            axum::extract::Path(project_id),
        )
        .await
        .expect("a repeated team delete should reuse its tombstone");

        let statements = format!("{:?}", db_log.into_transaction_log());
        assert!(statements.contains("FOR UPDATE"), "{statements}");
        assert!(statements.contains("project_host_bindings"), "{statements}");
        assert!(
            !statements.contains("project_host_bindings\".\"deleted_at\" IS NULL"),
            "{statements}"
        );
        assert!(!statements.contains("audit_events"), "{statements}");
        assert!(statements.contains("quota_events"), "{statements}");
    }

    #[tokio::test]
    async fn soft_delete_reuses_an_existing_tombstone_generation() {
        let project_id = Uuid::now_v7();
        let team_id = Uuid::now_v7();
        let stale = project::Model {
            id: project_id,
            team_id,
            created_by_user_id: None,
            slug: "deleted-project".to_owned(),
            name: "Deleted project".to_owned(),
            runtime: ProjectRuntime::Static,
            repository_url: None,
            default_branch: None,
            install_command: None,
            build_command: None,
            output_directory: None,
            source_config: serde_json::json!({}),
            build_config: serde_json::json!({}),
            archived_at: None,
            deleted_at: None,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
        };
        let generation = time::OffsetDateTime::from_unix_timestamp(42).unwrap();
        let tombstone = project::Model {
            deleted_at: Some(generation),
            updated_at: generation,
            ..stale.clone()
        };
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[tombstone.clone()]])
            .append_query_results([Vec::<project_host_binding::Model>::new()])
            .into_connection();

        let outcome = soft_delete_project_records(&db, stale).await.unwrap();

        assert!(!outcome.newly_deleted);
        assert_eq!(outcome.project.deleted_at, tombstone.deleted_at);
        assert!(outcome.bindings.is_empty());
        let statements = format!("{:?}", db.into_transaction_log());
        assert!(statements.contains("FOR UPDATE"), "{statements}");
        assert!(statements.contains("project_host_bindings"), "{statements}");
        assert!(
            !statements.contains("project_host_bindings\".\"deleted_at\" IS NULL"),
            "{statements}"
        );
        assert!(!statements.contains("UPDATE \"projects\""), "{statements}");
    }

    #[tokio::test]
    async fn tombstone_retry_ignores_bindings_from_older_delete_generations() {
        let project_id = Uuid::now_v7();
        let team_id = Uuid::now_v7();
        let generation_one = time::OffsetDateTime::from_unix_timestamp(10).unwrap();
        let generation_two = time::OffsetDateTime::from_unix_timestamp(20).unwrap();
        let tombstone = project::Model {
            id: project_id,
            team_id,
            created_by_user_id: None,
            slug: "generation-project".to_owned(),
            name: "Generation project".to_owned(),
            runtime: ProjectRuntime::Static,
            repository_url: None,
            default_branch: None,
            install_command: None,
            build_command: None,
            output_directory: None,
            source_config: serde_json::json!({}),
            build_config: serde_json::json!({}),
            archived_at: None,
            deleted_at: Some(generation_two),
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: generation_two,
        };
        let binding = |id: Uuid, deleted_at| project_host_binding::Model {
            id,
            project_id,
            team_id,
            host_source_id: None,
            host: format!("{id}.example.invalid"),
            kind: crate::infra::database::entity::HostBindingKind::Custom,
            environment: crate::infra::database::entity::HostBindingEnvironment::Preview,
            status: crate::infra::database::entity::HostBindingStatus::Active,
            failure_reason: None,
            is_primary: false,
            review_status: crate::infra::database::entity::HostReviewStatus::NotRequired,
            reviewed_by_user_id: None,
            reviewed_at: None,
            review_reason: None,
            deleted_at: Some(deleted_at),
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: deleted_at,
        };
        let old_binding = binding(Uuid::now_v7(), generation_one);
        let current_binding = binding(Uuid::now_v7(), generation_two);
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[tombstone.clone()]])
            .append_query_results([[old_binding, current_binding.clone()]])
            .into_connection();

        let outcome = soft_delete_project_records(&db, tombstone).await.unwrap();
        assert!(!outcome.newly_deleted);
        let statements = format!("{:?}", db.into_transaction_log());
        assert!(statements.contains("project_host_bindings"), "{statements}");
        assert!(statements.contains("deleted_at\\\" ="), "{statements}");
        assert!(
            !statements.contains("deleted_at\\\" IS NOT NULL"),
            "{statements}"
        );
    }

    #[tokio::test]
    async fn restore_loser_releases_reservation_without_auditing_or_consuming_quota() {
        let actor_id = Uuid::now_v7();
        let team_id = Uuid::now_v7();
        let tombstone_at = time::OffsetDateTime::from_unix_timestamp(42).unwrap();
        let stale_tombstone = project::Model {
            id: Uuid::now_v7(),
            team_id,
            created_by_user_id: None,
            slug: "restore-race".to_owned(),
            name: "Restore race".to_owned(),
            runtime: ProjectRuntime::Static,
            repository_url: None,
            default_branch: None,
            install_command: None,
            build_command: None,
            output_directory: None,
            source_config: serde_json::json!({}),
            build_config: serde_json::json!({}),
            archived_at: None,
            deleted_at: Some(tombstone_at),
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: tombstone_at,
        };
        let live_project = project::Model {
            deleted_at: None,
            ..stale_tombstone.clone()
        };
        let team = team::Model {
            id: team_id,
            slug: "restore-team".to_owned(),
            name: "Restore team".to_owned(),
            kind: crate::infra::database::entity::TeamKind::Personal,
            group_id: None,
            explicit_quota_plan_id: None,
            owner_user_id: Some(actor_id),
            deleted_at: None,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
        };
        let plan = crate::infra::database::entity::quota_plan::Model {
            id: Uuid::now_v7(),
            code: "default".to_owned(),
            name: "Default".to_owned(),
            description: None,
            is_default: true,
            enabled: true,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
        };
        let count_row = || {
            std::collections::BTreeMap::from([(
                "num_items".to_owned(),
                sea_orm::Value::BigInt(Some(0)),
            )])
        };
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[plan]])
            .append_query_results([
                Vec::<crate::infra::database::entity::quota_limit::Model>::new(),
            ])
            .append_query_results([[count_row()]])
            .append_query_results([[count_row()]])
            .append_query_results([[live_project]])
            .into_connection();
        let db_log = db.clone();
        let cache = grass_cache::CacheStore::Moka(grass_cache::MokaCache::connect());

        let result = restore_project_with_quota(
            &db,
            &cache,
            "test.restore.race",
            actor_id,
            &team,
            stale_tombstone,
            RestoreAuditContext::Team,
        )
        .await;

        assert!(matches!(result, Err(AppError::Conflict { .. })));
        assert_eq!(
            cache
                .get(&format!("quota:team:{team_id}:projects"))
                .await
                .unwrap()
                .as_deref(),
            Some("0")
        );
        assert_eq!(
            cache
                .get(&format!("quota:team:{team_id}:projects.static"))
                .await
                .unwrap()
                .as_deref(),
            Some("0")
        );
        let statements = format!("{:?}", db_log.into_transaction_log());
        assert!(statements.contains("FOR UPDATE"), "{statements}");
        assert!(!statements.contains("UPDATE \"projects\""), "{statements}");
        assert!(!statements.contains("audit_events"), "{statements}");
        assert!(!statements.contains("notifications"), "{statements}");
        assert!(!statements.contains("quota_events"), "{statements}");
    }
}

/// POST /api/v1/projects/{project_id}/archive
pub async fn archive(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.archive";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_admin(OP)?;
    let db = super::database(&state, OP)?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let project = projects::set_archived(&transaction, access.project, true)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_lifecycle_event(
        &transaction,
        session.data.user_id,
        "project.archived",
        &project,
        format!("/projects/{}", project.id),
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(
        json!({ "project": super::project_view(&project) }),
    ))
}

/// POST /api/v1/projects/{project_id}/unarchive
pub async fn unarchive(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.unarchive";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_admin(OP)?;
    let db = super::database(&state, OP)?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let project = projects::set_archived(&transaction, access.project, false)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_lifecycle_event(
        &transaction,
        session.data.user_id,
        "project.unarchived",
        &project,
        format!("/projects/{}", project.id),
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(
        json!({ "project": super::project_view(&project) }),
    ))
}

/// POST /api/v1/projects/{project_id}/delete — soft delete.
pub async fn delete(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.delete";
    let access = super::project_access(&state, &session, project_id, true, OP).await?;
    access.require_admin(OP)?;
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;

    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let deletion = soft_delete_project_records(&transaction, access.project)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    if deletion.newly_deleted {
        record_lifecycle_event(
            &transaction,
            session.data.user_id,
            "project.deleted",
            &deletion.project,
            "/projects".to_owned(),
        )
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    }
    let project = deletion.project;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    let warnings = if deletion.newly_deleted {
        finalize_deleted_project_resources(db, cache, OP, &project, &deletion.bindings).await?
    } else {
        release_deleted_project_quota(db, cache, OP, &project, &deletion.bindings).await?;
        Vec::new()
    };
    Ok(ok_response(
        json!({ "project": super::project_view(&project), "warnings": warnings }),
    ))
}

/// POST /api/v1/projects/{project_id}/restore
pub async fn restore(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.restore";
    let access = super::project_access(&state, &session, project_id, true, OP).await?;
    access.require_admin(OP)?;
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;
    let project = restore_project_with_quota(
        db,
        cache,
        OP,
        session.data.user_id,
        &access.team,
        access.project,
        RestoreAuditContext::Team,
    )
    .await?;

    Ok(ok_response(
        json!({ "project": super::project_view(&project) }),
    ))
}

#[derive(Clone, Copy)]
pub(crate) enum RestoreAuditContext {
    Team,
    PlatformAdmin,
}

pub(crate) async fn restore_project_with_quota(
    db: &sea_orm::DatabaseConnection,
    cache: &grass_cache::CacheStore,
    op: &'static str,
    actor_user_id: Uuid,
    team: &team::Model,
    project: project::Model,
    audit_context: RestoreAuditContext,
) -> Result<project::Model, AppError> {
    if project.deleted_at.is_none() {
        return Err(AppError::Conflict {
            op,
            message: "project is not deleted".to_owned(),
        });
    }

    let quota = QuotaService::new(db, cache);
    let reservation = quota
        .reserve(
            op,
            team,
            Some(actor_user_id),
            &[
                QuotaCharge::one(QuotaDimension::Projects),
                QuotaCharge::one(runtime_dimension(&project.runtime)),
            ],
        )
        .await?;

    let transaction = match db.begin().await {
        Ok(transaction) => transaction,
        Err(source) => {
            quota.rollback(reservation).await;
            return Err(AppError::Infrastructure {
                op,
                source: source.into(),
            });
        }
    };
    let current_project = match project::Entity::find()
        .filter(project::Column::Id.eq(project.id))
        .lock(LockType::Update)
        .one(&transaction)
        .await
    {
        Ok(Some(project)) => project,
        Ok(None) => {
            let _ = transaction.rollback().await;
            quota.rollback(reservation).await;
            return Err(AppError::NotFound {
                op,
                message: "project not found".to_owned(),
            });
        }
        Err(source) => {
            let _ = transaction.rollback().await;
            quota.rollback(reservation).await;
            return Err(AppError::Infrastructure {
                op,
                source: source.into(),
            });
        }
    };
    if current_project.deleted_at.is_none() {
        let _ = transaction.rollback().await;
        quota.rollback(reservation).await;
        return Err(AppError::Conflict {
            op,
            message: "project is not deleted".to_owned(),
        });
    }
    let project = match projects::restore(&transaction, current_project).await {
        Ok(project) => project,
        Err(source) => {
            let _ = transaction.rollback().await;
            quota.rollback(reservation).await;
            return Err(AppError::Infrastructure { op, source });
        }
    };
    let record_result = match audit_context {
        RestoreAuditContext::Team => {
            record_lifecycle_event(
                &transaction,
                actor_user_id,
                "project.restored",
                &project,
                format!("/projects/{}", project.id),
            )
            .await
        }
        RestoreAuditContext::PlatformAdmin => {
            audits::create_platform_audit_event(
                &transaction,
                CreateAuditEventParams {
                    actor_user_id: Some(actor_user_id),
                    actor_node_id: None,
                    team_id: Some(project.team_id),
                    action: "project.restored".to_owned(),
                    target_type: "project".to_owned(),
                    target_id: Some(project.id),
                    result: AuditEventResult::Success,
                    reason: None,
                    metadata: json!({ "platform_admin": true, "slug": project.slug }),
                },
            )
            .await
        }
    };
    if let Err(source) = record_result {
        let _ = transaction.rollback().await;
        quota.rollback(reservation).await;
        return Err(AppError::Infrastructure { op, source });
    }
    if matches!(audit_context, RestoreAuditContext::PlatformAdmin)
        && let Err(source) = notifications::create_project_notification(
            &transaction,
            notifications::CreateProjectNotification {
                project: &project,
                actor_user_id,
                action: "project.restored",
                reason: None,
                target_url: format!("/projects/{}", project.id),
            },
        )
        .await
    {
        let _ = transaction.rollback().await;
        quota.rollback(reservation).await;
        return Err(AppError::Infrastructure { op, source });
    }
    if let Err(source) = transaction.commit().await {
        quota.rollback(reservation).await;
        return Err(AppError::Infrastructure {
            op,
            source: source.into(),
        });
    }
    quota
        .commit(op, reservation, "project", Some(project.id))
        .await?;
    Ok(project)
}

#[derive(Deserialize)]
pub struct TransferTeamRequest {
    pub team_id: Uuid,
}

/// POST /api/v1/projects/{project_id}/transfer-team
pub async fn transfer_team(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
    Json(body): Json<TransferTeamRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.transfer_team";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_owner(OP)?;
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;

    if body.team_id == access.project.team_id {
        return Err(AppError::Validation {
            op: OP,
            message: "project already belongs to this team".to_owned(),
        });
    }

    let target_team = teams::get_by_id(db, body.team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "target team not found".to_owned(),
        })?;
    let target_role = teams::member_role(db, target_team.id, session.data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::Forbidden {
            op: OP,
            message: "not a member of the target team".to_owned(),
        })?;
    if !matches!(target_role, TeamMemberRole::Owner | TeamMemberRole::Admin) {
        return Err(AppError::Forbidden {
            op: OP,
            message: "admin role required in the target team".to_owned(),
        });
    }

    let charges = [
        QuotaCharge::one(QuotaDimension::Projects),
        QuotaCharge::one(runtime_dimension(&access.project.runtime)),
    ];
    let quota = QuotaService::new(db, cache);
    let reservation = quota
        .reserve(OP, &target_team, Some(session.data.user_id), &charges)
        .await?;

    let source_team_id = access.project.team_id;
    let project = match projects::transfer_team(db, access.project, target_team.id).await {
        Ok(project) => project,
        Err(source) => {
            quota.rollback(reservation).await;
            return Err(AppError::Infrastructure { op: OP, source });
        }
    };
    quota
        .commit(OP, reservation, "project", Some(project.id))
        .await?;
    quota
        .release(OP, source_team_id, &charges, "project", Some(project.id))
        .await?;
    record_lifecycle_audit(
        &state,
        session.data.user_id,
        target_team.id,
        "project.transferred",
        project.id,
        json!({ "from_team_id": source_team_id, "to_team_id": target_team.id }),
    )
    .await;

    Ok(ok_response(
        json!({ "project": super::project_view(&project) }),
    ))
}

/// POST /api/v1/projects/{project_id}/hard-delete
pub async fn hard_delete(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hard_delete";
    let access = super::project_access(&state, &session, project_id, true, OP).await?;
    access.require_owner(OP)?;
    if access.project.deleted_at.is_none() {
        return Err(AppError::Conflict {
            op: OP,
            message: "project must be soft deleted before it can be hard deleted".to_owned(),
        });
    }
    let db = super::database(&state, OP)?;

    projects::hard_delete(db, access.project.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_lifecycle_audit(
        &state,
        session.data.user_id,
        access.team.id,
        "project.hard_deleted",
        project_id,
        json!({}),
    )
    .await;

    Ok(ok_response(json!({ "ok": true })))
}
