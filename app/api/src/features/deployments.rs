use crate::AppState;
use axum::{
    Extension, Json, Router,
    extract::{Path, rejection::JsonRejection},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use axum_extra::extract::CookieJar;
use grass_worker_database::entities::{deployment, deployment_artifact};
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TransitionDeploymentStatus {
    Processing,
    Ready,
    Failed,
    Canceled,
}

#[derive(Debug, Deserialize)]
struct TransitionDeploymentRequest {
    status: TransitionDeploymentStatus,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ArtifactKindRequest {
    StaticSite,
    BuildLog,
}

#[derive(Debug, Deserialize)]
struct CreateDeploymentArtifactRequest {
    kind: ArtifactKindRequest,
    storage_path: String,
    #[serde(default)]
    checksum_sha256: Option<String>,
    #[serde(default)]
    size_bytes: Option<i64>,
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

#[derive(Debug, Serialize)]
struct DeploymentArtifactResponse {
    id: Uuid,
    deployment_id: Uuid,
    kind: &'static str,
    storage_path: String,
    checksum_sha256: Option<String>,
    size_bytes: Option<i64>,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl From<deployment_artifact::Model> for DeploymentArtifactResponse {
    fn from(value: deployment_artifact::Model) -> Self {
        Self {
            id: value.id,
            deployment_id: value.deployment_id,
            kind: artifact_kind_label(value.kind),
            storage_path: value.storage_path,
            checksum_sha256: value.checksum_sha256,
            size_bytes: value.size_bytes,
            created_at: value.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct DeploymentArtifactEnvelope {
    artifact: DeploymentArtifactResponse,
}

#[derive(Debug, Serialize)]
struct DeploymentArtifactsEnvelope {
    artifacts: Vec<DeploymentArtifactResponse>,
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
        .route(
            "/api/v1/projects/{id}/deployments/{deployment_id}/transition",
            post(transition_deployment),
        )
        .route(
            "/api/v1/projects/{id}/deployments/{deployment_id}/artifacts",
            post(create_deployment_artifact).get(list_deployment_artifacts),
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

async fn transition_deployment(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path((id, deployment_id)): Path<(String, String)>,
    payload: Result<Json<TransitionDeploymentRequest>, JsonRejection>,
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
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(error = %error, "transition deployment request rejected: invalid payload");
            return error_response(StatusCode::BAD_REQUEST, error.to_string());
        }
    };

    match state
        .deployments
        .transition_for_project(
            state.database.as_ref(),
            &actor,
            project_id,
            deployment_id,
            transition_status(payload.status),
        )
        .await
    {
        Ok(deployment) => Json(DeploymentEnvelope {
            deployment: deployment.into(),
        })
        .into_response(),
        Err(error) => deployment_error_response(error),
    }
}

async fn create_deployment_artifact(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path((id, deployment_id)): Path<(String, String)>,
    payload: Result<Json<CreateDeploymentArtifactRequest>, JsonRejection>,
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
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(error = %error, "create deployment artifact request rejected: invalid payload");
            return error_response(StatusCode::BAD_REQUEST, error.to_string());
        }
    };

    match state
        .deployments
        .register_artifact_for_project(
            state.database.as_ref(),
            &actor,
            project_id,
            deployment_id,
            crate::domain::deployment::RegisterDeploymentArtifactInput {
                kind: artifact_kind(payload.kind),
                storage_path: payload.storage_path,
                checksum_sha256: payload.checksum_sha256,
                size_bytes: payload.size_bytes,
            },
        )
        .await
    {
        Ok(artifact) => (
            StatusCode::CREATED,
            Json(DeploymentArtifactEnvelope {
                artifact: artifact.into(),
            }),
        )
            .into_response(),
        Err(error) => deployment_error_response(error),
    }
}

async fn list_deployment_artifacts(
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
        .list_artifacts_for_project(state.database.as_ref(), &actor, project_id, deployment_id)
        .await
    {
        Ok(artifacts) => Json(DeploymentArtifactsEnvelope {
            artifacts: artifacts
                .into_iter()
                .map(DeploymentArtifactResponse::from)
                .collect(),
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

fn transition_status(status: TransitionDeploymentStatus) -> deployment::DeploymentStatus {
    match status {
        TransitionDeploymentStatus::Processing => deployment::DeploymentStatus::Processing,
        TransitionDeploymentStatus::Ready => deployment::DeploymentStatus::Ready,
        TransitionDeploymentStatus::Failed => deployment::DeploymentStatus::Failed,
        TransitionDeploymentStatus::Canceled => deployment::DeploymentStatus::Canceled,
    }
}

fn artifact_kind(kind: ArtifactKindRequest) -> deployment_artifact::ArtifactKind {
    match kind {
        ArtifactKindRequest::StaticSite => deployment_artifact::ArtifactKind::StaticSite,
        ArtifactKindRequest::BuildLog => deployment_artifact::ArtifactKind::BuildLog,
    }
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

fn artifact_kind_label(kind: deployment_artifact::ArtifactKind) -> &'static str {
    match kind {
        deployment_artifact::ArtifactKind::StaticSite => "static_site",
        deployment_artifact::ArtifactKind::BuildLog => "build_log",
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
    use grass_worker_database::entities::{
        deployment, deployment_artifact, project, user, user_session,
    };
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
            active_deployment_id: None,
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

    fn sample_deployment_with_status(
        id: Uuid,
        project_id: Uuid,
        status: deployment::DeploymentStatus,
    ) -> deployment::Model {
        let created_at = Utc.with_ymd_and_hms(2026, 4, 28, 9, 0, 0).unwrap();
        let started_at = if matches!(
            status,
            deployment::DeploymentStatus::Processing
                | deployment::DeploymentStatus::Ready
                | deployment::DeploymentStatus::Failed
                | deployment::DeploymentStatus::Canceled
        ) {
            Some(created_at + Duration::minutes(5))
        } else {
            None
        };
        let finished_at = if matches!(
            status,
            deployment::DeploymentStatus::Ready
                | deployment::DeploymentStatus::Failed
                | deployment::DeploymentStatus::Canceled
        ) {
            Some(created_at + Duration::minutes(15))
        } else {
            None
        };

        deployment::Model {
            id,
            project_id,
            status,
            source_branch: Some("main".to_owned()),
            source_revision: Some("deadbeef".to_owned()),
            created_at,
            started_at,
            finished_at,
        }
    }

    fn sample_artifact(
        id: Uuid,
        deployment_id: Uuid,
        kind: deployment_artifact::ArtifactKind,
    ) -> deployment_artifact::Model {
        deployment_artifact::Model {
            id,
            deployment_id,
            kind,
            storage_path: "s3://artifacts/docs-site".to_owned(),
            checksum_sha256: Some("abc123".to_owned()),
            size_bytes: Some(1024),
            created_at: Utc.with_ymd_and_hms(2026, 4, 28, 9, 30, 0).unwrap(),
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

    #[tokio::test]
    async fn transition_deployment_returns_updated_envelope() {
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
            .append_query_results([[sample_deployment_with_status(
                deployment_id,
                project_id,
                deployment::DeploymentStatus::Pending,
            )]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .append_query_results([[sample_deployment_with_status(
                deployment_id,
                project_id,
                deployment::DeploymentStatus::Processing,
            )]])
            .into_connection();
        let app = install_deployment_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/projects/{project_id}/deployments/{deployment_id}/transition"
                    ))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(r#"{"status":"processing"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["deployment"]["id"], deployment_id.to_string());
        assert_eq!(json["deployment"]["status"], "processing");
        assert!(json["deployment"]["started_at"].is_string());
        assert!(json["deployment"]["finished_at"].is_null());
    }

    #[tokio::test]
    async fn create_deployment_artifact_returns_created_envelope() {
        let owner = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();
        let artifact_id = Uuid::new_v4();
        let artifact = sample_artifact(
            artifact_id,
            deployment_id,
            deployment_artifact::ArtifactKind::StaticSite,
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(owner.id, token)]])
            .append_query_results([[owner.clone()]])
            .append_query_results([[sample_project(
                project_id,
                owner.id,
                project::ProjectStatus::Active,
            )]])
            .append_query_results([[sample_deployment_with_status(
                deployment_id,
                project_id,
                deployment::DeploymentStatus::Processing,
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
                    .uri(format!(
                        "/api/v1/projects/{project_id}/deployments/{deployment_id}/artifacts"
                    ))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(
                        r#"{"kind":"static_site","storage_path":"s3://artifacts/docs-site","checksum_sha256":"abc123","size_bytes":1024}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["artifact"]["deployment_id"], deployment_id.to_string());
        assert_eq!(json["artifact"]["kind"], "static_site");
        assert_eq!(json["artifact"]["storage_path"], artifact.storage_path);
        assert_eq!(
            json["artifact"]["checksum_sha256"],
            artifact.checksum_sha256.unwrap()
        );
        assert_eq!(json["artifact"]["size_bytes"], 1024);
    }

    #[tokio::test]
    async fn transition_deployment_rejects_finishing_from_pending() {
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
            .append_query_results([[sample_deployment_with_status(
                deployment_id,
                project_id,
                deployment::DeploymentStatus::Pending,
            )]])
            .into_connection();
        let app = install_deployment_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/projects/{project_id}/deployments/{deployment_id}/transition"
                    ))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(r#"{"status":"ready"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["error"],
            "deployment must be processing before it can finish"
        );
    }

    #[tokio::test]
    async fn create_deployment_artifact_rejects_pending_deployment() {
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
            .append_query_results([[sample_deployment_with_status(
                deployment_id,
                project_id,
                deployment::DeploymentStatus::Pending,
            )]])
            .into_connection();
        let app = install_deployment_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v1/projects/{project_id}/deployments/{deployment_id}/artifacts"
                    ))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::from(
                        r#"{"kind":"build_log","storage_path":"s3://artifacts/build.log"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "deployment has not started");
    }

    #[tokio::test]
    async fn list_deployment_artifacts_returns_envelope() {
        let owner = sample_user(Uuid::new_v4(), false);
        let token = "session-token";
        let project_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();
        let artifact = sample_artifact(
            Uuid::new_v4(),
            deployment_id,
            deployment_artifact::ArtifactKind::BuildLog,
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_session(owner.id, token)]])
            .append_query_results([[owner.clone()]])
            .append_query_results([[sample_project(
                project_id,
                owner.id,
                project::ProjectStatus::Active,
            )]])
            .append_query_results([[sample_deployment_with_status(
                deployment_id,
                project_id,
                deployment::DeploymentStatus::Ready,
            )]])
            .append_query_results([[artifact.clone()]])
            .into_connection();
        let app = install_deployment_routes(Router::new(), AppState::new(database));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/projects/{project_id}/deployments/{deployment_id}/artifacts"
                    ))
                    .header(header::COOKIE, session_cookie(token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["artifacts"].as_array().unwrap().len(), 1);
        assert_eq!(json["artifacts"][0]["id"], artifact.id.to_string());
        assert_eq!(json["artifacts"][0]["kind"], "build_log");
    }
}
