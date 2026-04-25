use crate::AppState;
use axum::{
    Extension, Json, Router,
    extract::{Path, Query, rejection::JsonRejection, rejection::QueryRejection},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use axum_extra::extract::CookieJar;
use grass_worker_database::entities::project;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Deserialize)]
struct CreateProjectRequest {
    name: String,
    slug: String,
}

#[derive(Debug, Deserialize)]
struct UpdateProjectRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    slug: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ListProjectsQuery {
    #[serde(default)]
    status: Option<crate::domain::project::ProjectListStatus>,
}

#[derive(Debug, Deserialize)]
struct RestoreProjectRequest {
    status: crate::domain::project::RestoreProjectStatus,
}

#[derive(Debug, Deserialize)]
struct TransferProjectOwnerRequest {
    owner_email: String,
}

#[derive(Debug, Serialize)]
struct ProjectResponse {
    id: Uuid,
    owner_user_id: Uuid,
    slug: String,
    name: String,
    status: &'static str,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    archived_at: Option<chrono::DateTime<chrono::Utc>>,
    soft_deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<project::Model> for ProjectResponse {
    fn from(value: project::Model) -> Self {
        Self {
            id: value.id,
            owner_user_id: value.owner_user_id,
            slug: value.slug,
            name: value.name,
            status: project_status_label(value.status),
            created_at: value.created_at,
            updated_at: value.updated_at,
            archived_at: value.archived_at,
            soft_deleted_at: value.soft_deleted_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct ProjectEnvelope {
    project: ProjectResponse,
}

#[derive(Debug, Serialize)]
struct ProjectsEnvelope {
    projects: Vec<ProjectResponse>,
}

pub fn install_project_routes(router: Router, state: AppState) -> Router {
    let project_router = Router::new()
        .route("/api/v1/projects", post(create_project).get(list_projects))
        .route(
            "/api/v1/projects/{id}",
            get(get_project).patch(update_project),
        )
        .route("/api/v1/projects/{id}/archive", post(archive_project))
        .route("/api/v1/projects/{id}/unarchive", post(unarchive_project))
        .route(
            "/api/v1/projects/{id}/soft-delete",
            post(soft_delete_project),
        )
        .route("/api/v1/projects/{id}/restore", post(restore_project))
        .route(
            "/api/v1/projects/{id}/transfer-owner",
            post(transfer_project_owner),
        )
        .route(
            "/api/v1/projects/{id}/hard-delete",
            post(hard_delete_project),
        )
        .layer(Extension(state));

    router.merge(project_router)
}

async fn create_project(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    payload: Result<Json<CreateProjectRequest>, JsonRejection>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(error = %error, "create project request rejected: invalid payload");
            return error_response(StatusCode::BAD_REQUEST, error.to_string());
        }
    };

    match state
        .projects
        .create(
            state.database.as_ref(),
            &actor,
            payload.name.as_str(),
            payload.slug.as_str(),
        )
        .await
    {
        Ok(project) => (
            StatusCode::CREATED,
            Json(ProjectEnvelope {
                project: project.into(),
            }),
        )
            .into_response(),
        Err(error) => project_error_response(error),
    }
}

async fn list_projects(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    query: Result<Query<ListProjectsQuery>, QueryRejection>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let Query(query) = match query {
        Ok(query) => query,
        Err(error) => {
            tracing::warn!(error = %error, "list projects request rejected: invalid query");
            return error_response(StatusCode::BAD_REQUEST, error.to_string());
        }
    };
    let projects = match query.status {
        Some(status) => {
            state
                .projects
                .list(state.database.as_ref(), &actor, status)
                .await
        }
        None => {
            state
                .projects
                .list_default(state.database.as_ref(), &actor)
                .await
        }
    };

    match projects {
        Ok(projects) => Json(ProjectsEnvelope {
            projects: projects.into_iter().map(ProjectResponse::from).collect(),
        })
        .into_response(),
        Err(error) => project_error_response(error),
    }
}

async fn get_project(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let id = match parse_project_id(&id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };

    match state
        .projects
        .get(state.database.as_ref(), &actor, id)
        .await
    {
        Ok(project) => Json(ProjectEnvelope {
            project: project.into(),
        })
        .into_response(),
        Err(error) => project_error_response(error),
    }
}

async fn update_project(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    payload: Result<Json<UpdateProjectRequest>, JsonRejection>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let id = match parse_project_id(&id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(error = %error, "update project request rejected: invalid payload");
            return error_response(StatusCode::BAD_REQUEST, error.to_string());
        }
    };

    match state
        .projects
        .update(
            state.database.as_ref(),
            &actor,
            id,
            payload.name.as_deref(),
            payload.slug.as_deref(),
        )
        .await
    {
        Ok(project) => Json(ProjectEnvelope {
            project: project.into(),
        })
        .into_response(),
        Err(error) => project_error_response(error),
    }
}

async fn archive_project(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let id = match parse_project_id(&id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };

    match state
        .projects
        .archive(state.database.as_ref(), &actor, id)
        .await
    {
        Ok(project) => Json(ProjectEnvelope {
            project: project.into(),
        })
        .into_response(),
        Err(error) => project_error_response(error),
    }
}

async fn unarchive_project(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let id = match parse_project_id(&id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };

    match state
        .projects
        .unarchive(state.database.as_ref(), &actor, id)
        .await
    {
        Ok(project) => Json(ProjectEnvelope {
            project: project.into(),
        })
        .into_response(),
        Err(error) => project_error_response(error),
    }
}

async fn soft_delete_project(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let id = match parse_project_id(&id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };

    match state
        .projects
        .soft_delete(state.database.as_ref(), &actor, id)
        .await
    {
        Ok(project) => Json(ProjectEnvelope {
            project: project.into(),
        })
        .into_response(),
        Err(error) => project_error_response(error),
    }
}

async fn restore_project(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    payload: Result<Json<RestoreProjectRequest>, JsonRejection>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let id = match parse_project_id(&id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(error = %error, "restore project request rejected: invalid payload");
            return error_response(StatusCode::BAD_REQUEST, error.to_string());
        }
    };

    match state
        .projects
        .restore(state.database.as_ref(), &actor, id, payload.status)
        .await
    {
        Ok(project) => Json(ProjectEnvelope {
            project: project.into(),
        })
        .into_response(),
        Err(error) => project_error_response(error),
    }
}

async fn transfer_project_owner(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    payload: Result<Json<TransferProjectOwnerRequest>, JsonRejection>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let id = match parse_project_id(&id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "transfer project owner request rejected: invalid payload"
            );
            return error_response(StatusCode::BAD_REQUEST, error.to_string());
        }
    };

    match state
        .projects
        .transfer_owner(
            state.database.as_ref(),
            &actor,
            id,
            payload.owner_email.as_str(),
        )
        .await
    {
        Ok(project) => Json(ProjectEnvelope {
            project: project.into(),
        })
        .into_response(),
        Err(error) => project_error_response(error),
    }
}

async fn hard_delete_project(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let id = match parse_project_id(&id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };

    match state
        .projects
        .hard_delete(state.database.as_ref(), &actor, id)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => project_error_response(error),
    }
}

async fn authenticated_user(
    state: &AppState,
    jar: CookieJar,
) -> Result<crate::domain::auth::AuthenticatedUser, axum::response::Response> {
    let Some(session_cookie) = jar.get(crate::domain::auth::SESSION_COOKIE_NAME) else {
        return Err(auth_error_response(
            crate::domain::auth::AuthError::unauthorized("missing session"),
        ));
    };

    match state
        .auth
        .current_user(state.database.as_ref(), session_cookie.value())
        .await
    {
        Ok(user) => Ok(user),
        Err(error) => Err(auth_error_response(error)),
    }
}

fn auth_error_response(error: crate::domain::auth::AuthError) -> axum::response::Response {
    match error.kind() {
        crate::domain::auth::AuthErrorKind::Validation => {
            error_response(StatusCode::BAD_REQUEST, error.message())
        }
        crate::domain::auth::AuthErrorKind::Unauthorized => {
            error_response(StatusCode::UNAUTHORIZED, error.message())
        }
        crate::domain::auth::AuthErrorKind::Internal => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal auth error")
        }
    }
}

fn parse_project_id(raw: &str) -> Result<Uuid, &'static str> {
    Uuid::parse_str(raw).map_err(|_error| "invalid project id")
}

fn project_error_response(error: crate::domain::project::ProjectError) -> axum::response::Response {
    match error.kind() {
        crate::domain::project::ProjectErrorKind::Validation => {
            error_response(StatusCode::BAD_REQUEST, error.message())
        }
        crate::domain::project::ProjectErrorKind::NotFound => {
            error_response(StatusCode::NOT_FOUND, error.message())
        }
        crate::domain::project::ProjectErrorKind::Forbidden => {
            error_response(StatusCode::FORBIDDEN, error.message())
        }
        crate::domain::project::ProjectErrorKind::Conflict => {
            error_response(StatusCode::CONFLICT, error.message())
        }
        crate::domain::project::ProjectErrorKind::Internal => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal project error")
        }
    }
}

fn error_response(status: StatusCode, error: impl Into<String>) -> axum::response::Response {
    (
        status,
        Json(ErrorResponse {
            error: error.into(),
        }),
    )
        .into_response()
}

fn project_status_label(status: project::ProjectStatus) -> &'static str {
    match status {
        project::ProjectStatus::Active => "active",
        project::ProjectStatus::Archived => "archived",
        project::ProjectStatus::SoftDeleted => "soft_deleted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::auth::hash_session_token;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use chrono::{DateTime, Duration, TimeZone, Utc};
    use grass_worker_database::entities::{project, user, user_session};
    use sea_orm::{
        DatabaseBackend, DatabaseConnection, MockDatabase, MockDatabaseConnection, MockExecResult,
        Statement,
    };
    use std::sync::Arc;
    use tower::ServiceExt;
    use uuid::Uuid;

    fn sample_user(id: Uuid, is_admin: bool) -> user::Model {
        let created_at = Utc.with_ymd_and_hms(2026, 4, 23, 8, 0, 0).unwrap();

        user::Model {
            id,
            email: if is_admin {
                "admin@example.com".to_owned()
            } else {
                "owner@example.com".to_owned()
            },
            is_admin,
            is_initial_admin: is_admin,
            created_at,
            updated_at: created_at,
        }
    }

    fn sample_session(user_id: Uuid, token: &str) -> user_session::Model {
        let created_at = Utc.with_ymd_and_hms(2026, 4, 23, 8, 0, 0).unwrap();

        user_session::Model {
            id: Uuid::new_v4(),
            user_id,
            token_hash: hash_session_token(token),
            created_at,
            expires_at: created_at + Duration::days(7),
            revoked_at: None,
        }
    }

    fn sample_project(
        id: Uuid,
        owner_user_id: Uuid,
        status: project::ProjectStatus,
        slug: &str,
        name: &str,
    ) -> project::Model {
        let created_at = Utc.with_ymd_and_hms(2026, 4, 23, 8, 0, 0).unwrap();

        project::Model {
            id,
            owner_user_id,
            slug: slug.to_owned(),
            name: name.to_owned(),
            status: status.clone(),
            created_at,
            updated_at: created_at,
            archived_at: if status == project::ProjectStatus::Archived {
                Some(created_at + Duration::hours(1))
            } else {
                None
            },
            soft_deleted_at: if status == project::ProjectStatus::SoftDeleted {
                Some(created_at + Duration::hours(2))
            } else {
                None
            },
        }
    }

    fn session_cookie(token: &str) -> String {
        format!("{}={token}", crate::domain::auth::SESSION_COOKIE_NAME)
    }

    fn expected_status_label(status: &project::ProjectStatus) -> &'static str {
        match status {
            project::ProjectStatus::Active => "active",
            project::ProjectStatus::Archived => "archived",
            project::ProjectStatus::SoftDeleted => "soft_deleted",
        }
    }

    fn parse_utc_datetime(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn assert_project_payload(payload: &serde_json::Value, expected: &project::Model) {
        assert_eq!(payload["id"], expected.id.to_string());
        assert_eq!(payload["owner_user_id"], expected.owner_user_id.to_string());
        assert_eq!(payload["slug"], expected.slug);
        assert_eq!(payload["name"], expected.name);
        assert_eq!(payload["status"], expected_status_label(&expected.status));
        assert_eq!(
            parse_utc_datetime(payload["created_at"].as_str().unwrap()),
            expected.created_at
        );
        assert_eq!(
            parse_utc_datetime(payload["updated_at"].as_str().unwrap()),
            expected.updated_at
        );
        match expected.archived_at {
            Some(archived_at) => {
                assert_eq!(
                    parse_utc_datetime(payload["archived_at"].as_str().unwrap()),
                    archived_at
                );
            }
            None => assert!(payload["archived_at"].is_null()),
        }
        match expected.soft_deleted_at {
            Some(soft_deleted_at) => {
                assert_eq!(
                    parse_utc_datetime(payload["soft_deleted_at"].as_str().unwrap()),
                    soft_deleted_at
                );
            }
            None => assert!(payload["soft_deleted_at"].is_null()),
        }
    }

    fn project_select_statement(connection: Arc<MockDatabaseConnection>) -> Statement {
        DatabaseConnection::MockDatabaseConnection(connection)
            .into_transaction_log()
            .iter()
            .flat_map(|entry| entry.statements().iter())
            .find(|statement| statement.sql.contains("FROM \"projects\""))
            .cloned()
            .unwrap()
    }

    #[tokio::test]
    async fn create_project_returns_created_project() {
        let user = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(user.id, token)]])
            .append_query_results([[user.clone()]])
            .append_query_results([Vec::<project::Model>::new()])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(r#"{"name":"Docs Site","slug":"docs-site"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let payload = json.get("project").unwrap();

        assert_eq!(payload["owner_user_id"], user.id.to_string());
        assert_eq!(payload["slug"], "docs-site");
        assert_eq!(payload["name"], "Docs Site");
        assert_eq!(payload["status"], "active");
        assert!(Uuid::parse_str(payload["id"].as_str().unwrap()).is_ok());
        assert!(payload["created_at"].as_str().is_some());
        assert!(payload["updated_at"].as_str().is_some());
        assert!(payload["archived_at"].is_null());
        assert!(payload["soft_deleted_at"].is_null());
    }

    #[tokio::test]
    async fn list_projects_hides_soft_deleted_projects_from_non_admins() {
        let user = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(user.id, token)]])
            .append_query_results([[user]])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/projects?status=soft_deleted")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "forbidden");
    }

    #[tokio::test]
    async fn list_projects_returns_envelope_with_default_status_filter() {
        let user = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let active = sample_project(
            Uuid::new_v4(),
            user.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let archived = sample_project(
            Uuid::new_v4(),
            user.id,
            project::ProjectStatus::Archived,
            "docs-legacy",
            "Docs Legacy",
        );
        let connection = Arc::new(MockDatabaseConnection::new(
            MockDatabase::new(DatabaseBackend::Postgres)
                .append_query_results([[sample_session(user.id, token)]])
                .append_query_results([[user.clone()]])
                .append_query_results([vec![active.clone(), archived.clone()]]),
        ));
        let app = install_project_routes(
            Router::new(),
            AppState::new(DatabaseConnection::MockDatabaseConnection(
                connection.clone(),
            )),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/projects")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let projects = json["projects"].as_array().unwrap();
        assert_eq!(projects.len(), 2);
        assert_project_payload(&projects[0], &active);
        assert_project_payload(&projects[1], &archived);

        let transaction_log =
            DatabaseConnection::MockDatabaseConnection(connection).into_transaction_log();
        let statements = transaction_log
            .iter()
            .flat_map(|entry| entry.statements().iter())
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>();
        let project_select = statements
            .iter()
            .find(|statement| statement.contains("FROM \"projects\""))
            .unwrap();
        assert!(project_select.contains("'active'"));
        assert!(project_select.contains("'archived'"));
        assert!(!project_select.contains("'soft_deleted'"));
    }

    #[tokio::test]
    async fn list_projects_default_query_hides_soft_deleted_projects_for_admin() {
        let admin = sample_user(Uuid::new_v4(), true);
        let token = "session-token";
        let active = sample_project(
            Uuid::new_v4(),
            admin.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let archived = sample_project(
            Uuid::new_v4(),
            admin.id,
            project::ProjectStatus::Archived,
            "docs-legacy",
            "Docs Legacy",
        );
        let connection = Arc::new(MockDatabaseConnection::new(
            MockDatabase::new(DatabaseBackend::Postgres)
                .append_query_results([[sample_session(admin.id, token)]])
                .append_query_results([[admin.clone()]])
                .append_query_results([vec![active.clone(), archived.clone()]]),
        ));
        let app = install_project_routes(
            Router::new(),
            AppState::new(DatabaseConnection::MockDatabaseConnection(
                connection.clone(),
            )),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/projects")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let project_select = project_select_statement(connection);

        assert!(project_select.sql.contains("WHERE"));
        assert_eq!(
            project_select.values.as_ref().map(|values| values.0.len()),
            Some(2)
        );
        let rendered = project_select.to_string();
        assert!(rendered.contains("'active'"));
        assert!(rendered.contains("'archived'"));
        assert!(!rendered.contains("'soft_deleted'"));
    }

    #[tokio::test]
    async fn list_projects_all_query_includes_soft_deleted_projects_for_admin() {
        let admin = sample_user(Uuid::new_v4(), true);
        let token = "session-token";
        let active = sample_project(
            Uuid::new_v4(),
            admin.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let archived = sample_project(
            Uuid::new_v4(),
            admin.id,
            project::ProjectStatus::Archived,
            "docs-legacy",
            "Docs Legacy",
        );
        let soft_deleted = sample_project(
            Uuid::new_v4(),
            admin.id,
            project::ProjectStatus::SoftDeleted,
            "docs-retired",
            "Docs Retired",
        );
        let connection = Arc::new(MockDatabaseConnection::new(
            MockDatabase::new(DatabaseBackend::Postgres)
                .append_query_results([[sample_session(admin.id, token)]])
                .append_query_results([[admin.clone()]])
                .append_query_results([vec![
                    active.clone(),
                    archived.clone(),
                    soft_deleted.clone(),
                ]]),
        ));
        let app = install_project_routes(
            Router::new(),
            AppState::new(DatabaseConnection::MockDatabaseConnection(
                connection.clone(),
            )),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/projects?status=all")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let projects = json["projects"].as_array().unwrap();
        assert_eq!(projects.len(), 3);
        assert_project_payload(&projects[0], &active);
        assert_project_payload(&projects[1], &archived);
        assert_project_payload(&projects[2], &soft_deleted);

        let project_select = project_select_statement(connection);
        assert!(!project_select.sql.contains("WHERE"));
        assert_eq!(
            project_select.values.as_ref().map(|values| values.0.len()),
            Some(0)
        );
        let rendered = project_select.to_string();
        assert!(!rendered.contains("'active'"));
        assert!(!rendered.contains("'archived'"));
        assert!(!rendered.contains("'soft_deleted'"));
    }

    #[tokio::test]
    async fn list_projects_rejects_workspace_status_query() {
        let admin = sample_user(Uuid::new_v4(), true);
        let token = "session-token";
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(admin.id, token)]])
            .append_query_results([[admin]])
            .append_query_results([Vec::<project::Model>::new()])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/projects?status=workspace")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let error = json["error"].as_str().unwrap();
        assert!(error.contains("workspace"));
    }

    #[tokio::test]
    async fn get_project_returns_not_found_for_hidden_project() {
        let user = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let project = sample_project(
            Uuid::new_v4(),
            user.id,
            project::ProjectStatus::SoftDeleted,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(user.id, token)]])
            .append_query_results([[user]])
            .append_query_results([[project.clone()]])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/projects/{}", project.id))
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "project not found");
    }

    #[tokio::test]
    async fn get_project_returns_project_envelope() {
        let user = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let project = sample_project(
            Uuid::new_v4(),
            user.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(user.id, token)]])
            .append_query_results([[user]])
            .append_query_results([[project.clone()]])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/projects/{}", project.id))
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_project_payload(json.get("project").unwrap(), &project);
    }

    #[tokio::test]
    async fn get_project_returns_bad_request_for_malformed_project_id() {
        let user = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(user.id, token)]])
            .append_query_results([[user]])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/projects/not-a-uuid")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "invalid project id");
    }

    #[tokio::test]
    async fn update_project_returns_conflict_for_duplicate_slug() {
        let user = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let target_id = Uuid::new_v4();
        let target_project = sample_project(
            target_id,
            user.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let duplicated = sample_project(
            Uuid::new_v4(),
            Uuid::new_v4(),
            project::ProjectStatus::Active,
            "platform",
            "Platform",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(user.id, token)]])
            .append_query_results([[user]])
            .append_query_results([[target_project]])
            .append_query_results([[duplicated]])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/v1/projects/{target_id}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(r#"{"slug":"platform"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "slug already exists");
    }

    #[tokio::test]
    async fn update_project_returns_updated_project_envelope() {
        let user = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let existing = sample_project(
            project_id,
            user.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let mut updated = existing.clone();
        updated.name = "Docs Site V2".to_owned();
        updated.updated_at = existing.updated_at + Duration::hours(1);
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(user.id, token)]])
            .append_query_results([[user]])
            .append_query_results([[existing]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .append_query_results([[updated.clone()]])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/v1/projects/{project_id}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(r#"{"name":"Docs Site V2"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_project_payload(json.get("project").unwrap(), &updated);
    }

    #[tokio::test]
    async fn update_project_returns_bad_request_for_malformed_project_id() {
        let user = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(user.id, token)]])
            .append_query_results([[user]])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/v1/projects/not-a-uuid")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(r#"{"name":"Docs Site V2"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "invalid project id");
    }

    #[tokio::test]
    async fn archive_project_returns_archived_project_envelope() {
        let user = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let existing = sample_project(
            project_id,
            user.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let mut archived = existing.clone();
        archived.status = project::ProjectStatus::Archived;
        archived.updated_at = existing.updated_at + Duration::hours(1);
        archived.archived_at = Some(archived.updated_at);
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(user.id, token)]])
            .append_query_results([[user]])
            .append_query_results([[existing.clone()], [archived.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/archive"))
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_project_payload(json.get("project").unwrap(), &archived);
    }

    #[tokio::test]
    async fn unarchive_project_returns_active_project_envelope() {
        let user = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let existing = sample_project(
            project_id,
            user.id,
            project::ProjectStatus::Archived,
            "docs-site",
            "Docs Site",
        );
        let mut active = existing.clone();
        active.status = project::ProjectStatus::Active;
        active.updated_at = existing.updated_at + Duration::hours(1);
        active.archived_at = None;
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(user.id, token)]])
            .append_query_results([[user]])
            .append_query_results([[existing.clone()], [active.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/unarchive"))
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_project_payload(json.get("project").unwrap(), &active);
    }

    #[tokio::test]
    async fn soft_delete_project_returns_soft_deleted_project_envelope() {
        let user = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let existing = sample_project(
            project_id,
            user.id,
            project::ProjectStatus::Archived,
            "docs-site",
            "Docs Site",
        );
        let mut soft_deleted = existing.clone();
        soft_deleted.status = project::ProjectStatus::SoftDeleted;
        soft_deleted.updated_at = existing.updated_at + Duration::hours(1);
        soft_deleted.archived_at = None;
        soft_deleted.soft_deleted_at = Some(soft_deleted.updated_at);
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(user.id, token)]])
            .append_query_results([[user]])
            .append_query_results([[existing.clone()], [soft_deleted.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/soft-delete"))
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_project_payload(json.get("project").unwrap(), &soft_deleted);
    }

    #[tokio::test]
    async fn soft_delete_returns_not_found_for_non_owner() {
        let user = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let project = sample_project(
            project_id,
            Uuid::new_v4(),
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(user.id, token)]])
            .append_query_results([[user]])
            .append_query_results([[project]])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/soft-delete"))
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "project not found");
    }

    #[tokio::test]
    async fn restore_requires_admin() {
        let user = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let project = sample_project(
            project_id,
            user.id,
            project::ProjectStatus::SoftDeleted,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(user.id, token)]])
            .append_query_results([[user]])
            .append_query_results([[project]])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/restore"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(r#"{"status":"active"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "forbidden");
    }

    #[tokio::test]
    async fn restore_project_returns_archived_project_envelope() {
        let user = sample_user(Uuid::new_v4(), true);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let existing = sample_project(
            project_id,
            Uuid::new_v4(),
            project::ProjectStatus::SoftDeleted,
            "docs-site",
            "Docs Site",
        );
        let mut archived = existing.clone();
        archived.status = project::ProjectStatus::Archived;
        archived.updated_at = existing.updated_at + Duration::hours(1);
        archived.archived_at = Some(archived.updated_at);
        archived.soft_deleted_at = None;
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(user.id, token)]])
            .append_query_results([[user]])
            .append_query_results([[existing.clone()], [archived.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/restore"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(r#"{"status":"archived"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_project_payload(json.get("project").unwrap(), &archived);
    }

    #[tokio::test]
    async fn restore_project_returns_active_project_envelope() {
        let user = sample_user(Uuid::new_v4(), true);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let existing = sample_project(
            project_id,
            Uuid::new_v4(),
            project::ProjectStatus::SoftDeleted,
            "docs-site",
            "Docs Site",
        );
        let mut active = existing.clone();
        active.status = project::ProjectStatus::Active;
        active.updated_at = existing.updated_at + Duration::hours(1);
        active.archived_at = None;
        active.soft_deleted_at = None;
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(user.id, token)]])
            .append_query_results([[user]])
            .append_query_results([[existing.clone()], [active.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/restore"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(r#"{"status":"active"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_project_payload(json.get("project").unwrap(), &active);
    }

    #[tokio::test]
    async fn transfer_owner_requires_existing_target_user() {
        let admin = sample_user(Uuid::new_v4(), true);
        let token = "session-token";
        let project = sample_project(
            Uuid::new_v4(),
            admin.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(admin.id, token)]])
            .append_query_results([[admin.clone()]])
            .append_query_results([[project.clone()]])
            .append_query_results([Vec::<grass_worker_database::entities::user::Model>::new()])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{}/transfer-owner", project.id))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(r#"{"owner_email":"missing@example.com"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "user not found");
    }

    #[tokio::test]
    async fn transfer_owner_returns_updated_project_envelope() {
        let admin = sample_user(Uuid::new_v4(), true);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let existing = sample_project(
            project_id,
            Uuid::new_v4(),
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let target_user = grass_worker_database::entities::user::Model {
            id: Uuid::new_v4(),
            email: "new-owner@example.com".to_owned(),
            is_admin: false,
            is_initial_admin: false,
            created_at: existing.created_at,
            updated_at: existing.created_at,
        };
        let mut transferred = existing.clone();
        transferred.owner_user_id = target_user.id;
        transferred.updated_at = existing.updated_at + Duration::hours(1);
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(admin.id, token)]])
            .append_query_results([[admin]])
            .append_query_results([[existing.clone()]])
            .append_query_results([[target_user]])
            .append_query_results([[transferred.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/transfer-owner"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(r#"{"owner_email":"new-owner@example.com"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_project_payload(json.get("project").unwrap(), &transferred);
    }

    #[tokio::test]
    async fn transfer_owner_rejects_blank_owner_email() {
        let admin = sample_user(Uuid::new_v4(), true);
        let token = "session-token";
        let project = sample_project(
            Uuid::new_v4(),
            admin.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(admin.id, token)]])
            .append_query_results([[admin]])
            .append_query_results([[project.clone()]])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{}/transfer-owner", project.id))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from("{\"owner_email\":\"   \"}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "owner_email is required");
    }

    #[tokio::test]
    async fn owner_can_transfer_project_to_existing_user() {
        let owner = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let existing = sample_project(
            project_id,
            owner.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let target_user = grass_worker_database::entities::user::Model {
            id: Uuid::new_v4(),
            email: "new-owner@example.com".to_owned(),
            is_admin: false,
            is_initial_admin: false,
            created_at: existing.created_at,
            updated_at: existing.created_at,
        };
        let mut transferred = existing.clone();
        transferred.owner_user_id = target_user.id;
        transferred.updated_at = existing.updated_at + Duration::hours(1);
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(owner.id, token)]])
            .append_query_results([[owner]])
            .append_query_results([[existing.clone()]])
            .append_query_results([[target_user]])
            .append_query_results([[transferred.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/transfer-owner"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(r#"{"owner_email":"new-owner@example.com"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_project_payload(json.get("project").unwrap(), &transferred);
    }

    #[tokio::test]
    async fn transfer_owner_rejects_soft_deleted_project() {
        let admin = sample_user(Uuid::new_v4(), true);
        let token = "session-token";
        let project = sample_project(
            Uuid::new_v4(),
            Uuid::new_v4(),
            project::ProjectStatus::SoftDeleted,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(admin.id, token)]])
            .append_query_results([[admin]])
            .append_query_results([[project.clone()]])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{}/transfer-owner", project.id))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(r#"{"owner_email":"new-owner@example.com"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["error"],
            "soft deleted projects must be restored before transfer"
        );
    }

    #[tokio::test]
    async fn hard_delete_requires_soft_deleted_status_and_admin() {
        let admin = sample_user(Uuid::new_v4(), true);
        let token = "session-token";
        let project = sample_project(
            Uuid::new_v4(),
            admin.id,
            project::ProjectStatus::Archived,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(admin.id, token)]])
            .append_query_results([[admin]])
            .append_query_results([[project.clone()]])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{}/hard-delete", project.id))
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["error"],
            "project must be soft deleted before hard delete"
        );
    }

    #[tokio::test]
    async fn hard_delete_requires_admin_for_soft_deleted_project() {
        let owner = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let project = sample_project(
            Uuid::new_v4(),
            owner.id,
            project::ProjectStatus::SoftDeleted,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(owner.id, token)]])
            .append_query_results([[owner]])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{}/hard-delete", project.id))
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "forbidden");
    }

    #[tokio::test]
    async fn hard_delete_returns_no_content_for_admin_soft_deleted_project() {
        let admin = sample_user(Uuid::new_v4(), true);
        let token = "session-token";
        let project = sample_project(
            Uuid::new_v4(),
            Uuid::new_v4(),
            project::ProjectStatus::SoftDeleted,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(admin.id, token)]])
            .append_query_results([[admin]])
            .append_query_results([[project.clone()]])
            .append_query_results([
                Vec::<grass_worker_database::entities::deployment::Model>::new(),
            ])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{}/hard-delete", project.id))
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn hard_delete_rejects_projects_with_deployments() {
        let admin = sample_user(Uuid::new_v4(), true);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let project = sample_project(
            project_id,
            Uuid::new_v4(),
            project::ProjectStatus::SoftDeleted,
            "docs-site",
            "Docs Site",
        );
        let deployment = grass_worker_database::entities::deployment::Model {
            id: Uuid::new_v4(),
            project_id,
            status: grass_worker_database::entities::deployment::DeploymentStatus::Pending,
            source_branch: None,
            source_revision: None,
            created_at: project.created_at,
            started_at: None,
            finished_at: None,
        };
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(admin.id, token)]])
            .append_query_results([[admin]])
            .append_query_results([[project.clone()]])
            .append_query_results([[deployment]])
            .into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/hard-delete"))
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "project still has deployments");
    }

    #[tokio::test]
    async fn project_resource_route_does_not_allow_delete() {
        let database = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/projects/{}", Uuid::new_v4()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn unauthenticated_project_request_returns_unauthorized() {
        let database = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
        let app = install_project_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/projects")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "missing session");
    }
}
