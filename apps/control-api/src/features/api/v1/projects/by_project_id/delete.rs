use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use uuid::Uuid;

use crate::{
    domain::{
        project_lifecycle::{
            finalize_deleted_project_resources, record_lifecycle_event,
            release_deleted_project_quota, soft_delete_project_records,
        },
        projects,
    },
    infra::{
        database::entity::project,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/projects/{project_id}/delete", axum::routing::post(delete))
}

fn project_view(project: &project::Model) -> ProjectResponse {
    ProjectResponse {
        id: project.id,
        team_id: project.team_id,
        slug: project.slug.clone(),
        name: project.name.clone(),
        runtime: projects::runtime_value(&project.runtime),
        repository_url: project.repository_url.clone(),
        default_branch: project.default_branch.clone(),
        install_command: project.install_command.clone(),
        build_command: project.build_command.clone(),
        output_directory: project.output_directory.clone(),
        source_config: project.source_config.clone(),
        build_config: project.build_config.clone(),
        archived_at: project.archived_at,
        deleted_at: project.deleted_at,
        created_at: project.created_at,
        updated_at: project.updated_at,
    }
}

/// POST /api/v1/projects/{project_id}/delete — soft delete.
async fn delete(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.delete";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::IncludingDeleted,
        OP,
    )
    .await?;
    access.require_admin(OP)?;
    let db = crate::infra::http::database(&state, OP)?;
    let cache = crate::infra::http::cache(&state, OP)?;

    let transaction = crate::infra::audit::AuditTransaction::begin(db)
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
        let platform_secret = state.config.read().unwrap().secrets.secret_key.clone();
        finalize_deleted_project_resources(
            db,
            cache,
            &platform_secret,
            OP,
            &project,
            &deletion.bindings,
        )
        .await?
    } else {
        release_deleted_project_quota(db, cache, OP, &project, &deletion.bindings).await?;
        Vec::new()
    };
    Ok(ok_response(DeleteResponse {
        project: project_view(&project),
        warnings,
    }))
}

#[derive(serde::Serialize)]
struct ProjectResponse {
    id: uuid::Uuid,
    team_id: uuid::Uuid,
    slug: String,
    name: String,
    runtime: &'static str,
    repository_url: Option<String>,
    default_branch: Option<String>,
    install_command: Option<String>,
    build_command: Option<String>,
    output_directory: Option<String>,
    source_config: serde_json::Value,
    build_config: serde_json::Value,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    archived_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    deleted_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    updated_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct DeleteResponse {
    project: ProjectResponse,
    warnings: Vec<crate::domain::project_lifecycle::ProjectCleanupWarning>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::database::entity::ProjectRuntime;
    use crate::infra::database::entity::TeamMemberRole;
    use crate::infra::database::entity::project;
    use crate::infra::database::entity::project_host_binding;
    use crate::infra::database::entity::team;
    use crate::infra::http::extractors::Session;
    use tower::ServiceExt;
    use uuid::Uuid;
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
            avatar_version: None,
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
            region: "default".to_owned(),
            kind: crate::infra::database::entity::HostBindingKind::Custom,
            environment: crate::infra::database::entity::HostBindingEnvironment::Preview,
            status: crate::infra::database::entity::HostBindingStatus::Active,
            failure_reason: None,
            is_primary: false,
            review_status: crate::infra::database::entity::HostReviewStatus::NotRequired,
            reviewed_by_user_id: None,
            reviewed_at: None,
            review_reason: None,
            ownership_status: "pending".to_owned(),
            ownership_checked_at: None,
            ownership_error: None,
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
                auth_version: 1,
                user_id: actor_id,
                created_at: time::OffsetDateTime::UNIX_EPOCH,
                last_accessed_at: time::OffsetDateTime::UNIX_EPOCH,
            },
            session_id: "team-session".to_owned(),
        };

        let response = router()
            .with_state(state)
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/projects/{project_id}/delete"))
                    .extension(Some((session.session_id, session.data)))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);

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
}
