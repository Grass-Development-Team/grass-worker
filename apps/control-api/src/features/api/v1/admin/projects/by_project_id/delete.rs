use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use uuid::Uuid;

use crate::{
    infra::{
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/projects/{project_id}/delete", axum::routing::post(remove))
}

/// POST /api/v1/admin/projects/{project_id}/delete — soft delete. Serving
/// and deployments stop resolving; restore stays possible through the
/// team-level restore endpoint.
pub async fn remove(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let warnings = crate::domain::admin_projects::remove(&state, data.user_id, project_id).await?;
    Ok(ok_response(RemoveResponse {
        deleted: true,
        warnings,
    }))
}

#[derive(serde::Serialize)]
struct RemoveResponse {
    deleted: bool,
    warnings: Vec<crate::domain::project_lifecycle::ProjectCleanupWarning>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::database::entity::ProjectRuntime;
    use crate::infra::database::entity::project;
    use crate::infra::database::entity::project_host_binding;
    use time::OffsetDateTime;
    use uuid::Uuid;

    fn inventory_project(
        team_id: Uuid,
        archived_at: Option<OffsetDateTime>,
        deleted_at: Option<OffsetDateTime>,
    ) -> project::Model {
        let now = OffsetDateTime::UNIX_EPOCH;
        project::Model {
            id: Uuid::now_v7(),
            team_id,
            created_by_user_id: None,
            slug: Uuid::now_v7().to_string(),
            name: "Inventory project".to_owned(),
            runtime: ProjectRuntime::Static,
            repository_url: None,
            default_branch: None,
            install_command: None,
            build_command: None,
            output_directory: None,
            source_config: serde_json::json!({}),
            build_config: serde_json::json!({}),
            archived_at,
            deleted_at,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn admin_delete_retry_releases_quota_without_repeating_lifecycle_event() {
        use axum::{
            body::to_bytes,
            extract::{Path, State},
            response::IntoResponse,
        };

        let actor_id = Uuid::now_v7();
        let team_id = Uuid::now_v7();
        let tombstone = inventory_project(
            team_id,
            None,
            Some(OffsetDateTime::from_unix_timestamp(42).unwrap()),
        );
        let project_id = tombstone.id;
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
            created_at: OffsetDateTime::UNIX_EPOCH,
        };
        let counter =
            |dimension: &str| crate::infra::database::entity::quota_usage_counter::Model {
                id: Uuid::now_v7(),
                team_id,
                dimension: dimension.to_owned(),
                used_value: 0,
                period_start: None,
                period_end: None,
                updated_at: OffsetDateTime::UNIX_EPOCH,
            };
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[tombstone.clone()]])
            .append_query_results([[tombstone]])
            .append_query_results([Vec::<project_host_binding::Model>::new()])
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
        let session = crate::infra::http::extractors::Session {
            data: grass_session::SessionData {
                auth_version: 1,
                user_id: actor_id,
                created_at: OffsetDateTime::UNIX_EPOCH,
                last_accessed_at: OffsetDateTime::UNIX_EPOCH,
            },
            session_id: "admin-session".to_owned(),
        };

        let response = remove(State(state), session, Path(project_id))
            .await
            .expect("a repeated admin delete should reuse its tombstone")
            .into_response();
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(body["data"]["deleted"], true);
        assert_eq!(body["data"]["warnings"], serde_json::json!([]));
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
