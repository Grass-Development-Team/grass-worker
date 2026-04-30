use crate::AppState;
use axum::{
    Extension, Json, Router,
    extract::{Path, rejection::JsonRejection},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use axum_extra::extract::CookieJar;
use grass_worker_database::entities::deployment;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Deserialize, Default)]
struct CreateDeploymentRequest {
    #[serde(default)]
    source_branch: Option<String>,
    #[serde(default)]
    source_revision: Option<String>,
}

#[derive(Debug, Serialize)]
struct DeploymentResponse {
    id: Uuid,
    project_id: Uuid,
    status: &'static str,
    source_branch: Option<String>,
    source_revision: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<deployment::Model> for DeploymentResponse {
    fn from(value: deployment::Model) -> Self {
        Self {
            id: value.id,
            project_id: value.project_id,
            status: deployment_status_label(value.status),
            source_branch: value.source_branch,
            source_revision: value.source_revision,
            created_at: value.created_at,
            started_at: value.started_at,
            finished_at: value.finished_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct DeploymentEnvelope {
    deployment: DeploymentResponse,
}

#[derive(Debug, Serialize)]
struct DeploymentsEnvelope {
    deployments: Vec<DeploymentResponse>,
}

pub fn install_deployment_routes(router: Router, state: AppState) -> Router {
    let deployment_router = Router::new()
        .route(
            "/api/v1/projects/{id}/deployments",
            post(create_deployment).get(list_deployments),
        )
        .route(
            "/api/v1/projects/{id}/deployments/{deployment_id}",
            get(get_deployment),
        )
        .layer(Extension(state));

    router.merge(deployment_router)
}

async fn create_deployment(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    payload: Result<Json<CreateDeploymentRequest>, JsonRejection>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let project_id = match parse_project_id(&id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(error = %error, "create deployment request rejected: invalid payload");
            return error_response(StatusCode::BAD_REQUEST, error.to_string());
        }
    };

    match state
        .deployments
        .create(
            state.database.as_ref(),
            &actor,
            project_id,
            payload.source_branch.as_deref(),
            payload.source_revision.as_deref(),
        )
        .await
    {
        Ok(deployment) => (
            StatusCode::CREATED,
            Json(DeploymentEnvelope {
                deployment: deployment.into(),
            }),
        )
            .into_response(),
        Err(error) => deployment_error_response(error),
    }
}

async fn list_deployments(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let project_id = match parse_project_id(&id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };

    match state
        .deployments
        .list_for_project(state.database.as_ref(), &actor, project_id)
        .await
    {
        Ok(deployments) => Json(DeploymentsEnvelope {
            deployments: deployments
                .into_iter()
                .map(DeploymentResponse::from)
                .collect(),
        })
        .into_response(),
        Err(error) => deployment_error_response(error),
    }
}

async fn get_deployment(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path((id, deployment_id)): Path<(String, String)>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let project_id = match parse_project_id(&id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    let deployment_id = match parse_deployment_id(&deployment_id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };

    match state
        .deployments
        .get_for_project(state.database.as_ref(), &actor, project_id, deployment_id)
        .await
    {
        Ok(deployment) => Json(DeploymentEnvelope {
            deployment: deployment.into(),
        })
        .into_response(),
        Err(error) => deployment_error_response(error),
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

fn parse_deployment_id(raw: &str) -> Result<Uuid, &'static str> {
    Uuid::parse_str(raw).map_err(|_error| "invalid deployment id")
}

fn deployment_error_response(
    error: crate::domain::deployment::DeploymentError,
) -> axum::response::Response {
    match error.kind() {
        crate::domain::deployment::DeploymentErrorKind::Validation => {
            error_response(StatusCode::BAD_REQUEST, error.message())
        }
        crate::domain::deployment::DeploymentErrorKind::NotFound => {
            error_response(StatusCode::NOT_FOUND, error.message())
        }
        crate::domain::deployment::DeploymentErrorKind::Forbidden => {
            error_response(StatusCode::FORBIDDEN, error.message())
        }
        crate::domain::deployment::DeploymentErrorKind::Conflict => {
            error_response(StatusCode::CONFLICT, error.message())
        }
        crate::domain::deployment::DeploymentErrorKind::Internal => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal deployment error",
        ),
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

fn deployment_status_label(status: deployment::DeploymentStatus) -> &'static str {
    match status {
        deployment::DeploymentStatus::Pending => "pending",
        deployment::DeploymentStatus::Processing => "processing",
        deployment::DeploymentStatus::Ready => "ready",
        deployment::DeploymentStatus::Failed => "failed",
        deployment::DeploymentStatus::Canceled => "canceled",
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
    use chrono::{Duration, TimeZone, Utc};
    use grass_worker_database::entities::{deployment, project, user, user_session};
    use sea_orm::{DatabaseBackend, MockDatabase, MockExecResult};
    use tower::ServiceExt;
    use uuid::Uuid;

    fn sample_user(id: Uuid, is_admin: bool) -> user::Model {
        let created_at = Utc.with_ymd_and_hms(2026, 4, 28, 8, 0, 0).unwrap();
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
        let created_at = Utc::now();
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
    ) -> project::Model {
        let created_at = Utc.with_ymd_and_hms(2026, 4, 28, 8, 0, 0).unwrap();
        project::Model {
            id,
            owner_user_id,
            slug: "docs-site".to_owned(),
            name: "Docs Site".to_owned(),
            status: status.clone(),
            created_at,
            updated_at: created_at,
            archived_at: if status == project::ProjectStatus::Archived {
                Some(created_at + Duration::hours(1))
            } else {
                None
            },
            soft_deleted_at: None,
        }
    }

    fn sample_deployment(id: Uuid, project_id: Uuid) -> deployment::Model {
        let created_at = Utc.with_ymd_and_hms(2026, 4, 28, 9, 0, 0).unwrap();
        deployment::Model {
            id,
            project_id,
            status: deployment::DeploymentStatus::Pending,
            source_branch: Some("main".to_owned()),
            source_revision: Some("deadbeef".to_owned()),
            created_at,
            started_at: None,
            finished_at: None,
        }
    }

    fn session_cookie(token: &str) -> String {
        format!("{}={token}", crate::domain::auth::SESSION_COOKIE_NAME)
    }

    #[tokio::test]
    async fn create_deployment_returns_created_envelope() {
        let owner = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(owner.id, token)]])
            .append_query_results([[owner.clone()]])
            .append_query_results([[sample_project(
                project_id,
                owner.id,
                project::ProjectStatus::Active,
            )]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let app = install_deployment_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/deployments"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(
                        r#"{"source_branch":"main","source_revision":"deadbeef"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["deployment"]["project_id"], project_id.to_string());
        assert_eq!(json["deployment"]["status"], "pending");
        assert_eq!(json["deployment"]["source_branch"], "main");
        assert_eq!(json["deployment"]["source_revision"], "deadbeef");
    }

    #[tokio::test]
    async fn create_deployment_rejects_archived_project() {
        let owner = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(owner.id, token)]])
            .append_query_results([[owner.clone()]])
            .append_query_results([[sample_project(
                project_id,
                owner.id,
                project::ProjectStatus::Archived,
            )]])
            .into_connection();
        let app = install_deployment_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/deployments"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(r#"{}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "project is not active");
    }

    #[tokio::test]
    async fn list_project_deployments_returns_envelope() {
        let owner = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(owner.id, token)]])
            .append_query_results([[owner.clone()]])
            .append_query_results([[sample_project(
                project_id,
                owner.id,
                project::ProjectStatus::Active,
            )]])
            .append_query_results([[sample_deployment(deployment_id, project_id)]])
            .into_connection();
        let app = install_deployment_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/projects/{project_id}/deployments"))
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["deployments"].as_array().unwrap().len(), 1);
        assert_eq!(json["deployments"][0]["id"], deployment_id.to_string());
    }

    #[tokio::test]
    async fn get_project_deployment_returns_not_found_for_mismatched_project() {
        let owner = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let other_project_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(owner.id, token)]])
            .append_query_results([[owner.clone()]])
            .append_query_results([[sample_project(
                project_id,
                owner.id,
                project::ProjectStatus::Active,
            )]])
            .append_query_results([[sample_deployment(deployment_id, other_project_id)]])
            .into_connection();
        let app = install_deployment_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/projects/{project_id}/deployments/{deployment_id}"
                    ))
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "deployment not found");
    }
}
