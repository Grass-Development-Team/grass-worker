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

#[derive(Debug, Deserialize)]
struct ActivateReleaseRequest {
    deployment_id: Uuid,
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
struct ReleaseResponse {
    project_id: Uuid,
    project_slug: String,
    primary_host: Option<String>,
    active_deployment_id: Option<Uuid>,
    active_deployment: Option<DeploymentResponse>,
    rollback_deployment_id: Option<Uuid>,
}

impl From<crate::domain::release::ReleaseState> for ReleaseResponse {
    fn from(value: crate::domain::release::ReleaseState) -> Self {
        Self {
            project_id: value.project_id,
            project_slug: value.project_slug,
            primary_host: value.primary_host,
            active_deployment_id: value.active_deployment_id,
            active_deployment: value.active_deployment.map(DeploymentResponse::from),
            rollback_deployment_id: value.rollback_deployment_id,
        }
    }
}

#[derive(Debug, Serialize)]
struct ReleaseEnvelope {
    release: ReleaseResponse,
}

pub fn install_release_routes(router: Router, state: AppState) -> Router {
    let release_router = Router::new()
        .route("/api/v1/projects/{id}/release", get(get_release))
        .route(
            "/api/v1/projects/{id}/release/activate",
            post(activate_release),
        )
        .route(
            "/api/v1/projects/{id}/release/rollback",
            post(rollback_release),
        )
        .layer(Extension(state));

    router.merge(release_router)
}

async fn get_release(
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
        .releases
        .get_for_project(state.database.as_ref(), &actor, project_id)
        .await
    {
        Ok(release) => Json(ReleaseEnvelope {
            release: release.into(),
        })
        .into_response(),
        Err(error) => release_error_response(error),
    }
}

async fn activate_release(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    payload: Result<Json<ActivateReleaseRequest>, JsonRejection>,
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
            tracing::warn!(error = %error, "activate release request rejected: invalid payload");
            return error_response(StatusCode::BAD_REQUEST, error.to_string());
        }
    };

    match state
        .releases
        .activate_for_project(
            state.database.as_ref(),
            &actor,
            project_id,
            payload.deployment_id,
        )
        .await
    {
        Ok(release) => Json(ReleaseEnvelope {
            release: release.into(),
        })
        .into_response(),
        Err(error) => release_error_response(error),
    }
}

async fn rollback_release(
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
        .releases
        .rollback_for_project(state.database.as_ref(), &actor, project_id)
        .await
    {
        Ok(release) => Json(ReleaseEnvelope {
            release: release.into(),
        })
        .into_response(),
        Err(error) => release_error_response(error),
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

fn release_error_response(error: crate::domain::release::ReleaseError) -> axum::response::Response {
    match error.kind() {
        crate::domain::release::ReleaseErrorKind::Validation => {
            error_response(StatusCode::BAD_REQUEST, error.message())
        }
        crate::domain::release::ReleaseErrorKind::NotFound => {
            error_response(StatusCode::NOT_FOUND, error.message())
        }
        crate::domain::release::ReleaseErrorKind::Forbidden => {
            error_response(StatusCode::FORBIDDEN, error.message())
        }
        crate::domain::release::ReleaseErrorKind::Conflict => {
            error_response(StatusCode::CONFLICT, error.message())
        }
        crate::domain::release::ReleaseErrorKind::Internal => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal release error")
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

fn deployment_status_label(status: deployment::DeploymentStatus) -> &'static str {
    match status {
        deployment::DeploymentStatus::Pending => "pending",
        deployment::DeploymentStatus::Processing => "processing",
        deployment::DeploymentStatus::Ready => "ready",
        deployment::DeploymentStatus::Failed => "failed",
        deployment::DeploymentStatus::Canceled => "canceled",
    }
}
