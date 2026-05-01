use crate::AppState;
use axum::{
    Extension, Json, Router,
    extract::{Path, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use axum_extra::extract::CookieJar;
use grass_worker_database::entities::deployment;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Component, Path as FilePath},
};
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
    site_url: String,
    active_deployment_id: Option<Uuid>,
    active_deployment: Option<DeploymentResponse>,
    rollback_deployment_id: Option<Uuid>,
}

impl From<crate::domain::release::ReleaseState> for ReleaseResponse {
    fn from(value: crate::domain::release::ReleaseState) -> Self {
        Self {
            project_id: value.project_id,
            site_url: format!("/sites/{}", value.project_slug),
            project_slug: value.project_slug,
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
        .route("/sites/{project_slug}", get(serve_site_root))
        .route("/sites/{project_slug}/{*path}", get(serve_site_path))
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

async fn serve_site_root(
    Extension(state): Extension<AppState>,
    Path(project_slug): Path<String>,
) -> axum::response::Response {
    serve_site_response(state, project_slug, "").await
}

async fn serve_site_path(
    Extension(state): Extension<AppState>,
    Path((project_slug, path)): Path<(String, String)>,
) -> axum::response::Response {
    serve_site_response(state, project_slug, path.as_str()).await
}

async fn serve_site_response(
    state: AppState,
    project_slug: String,
    request_path: &str,
) -> axum::response::Response {
    let release = match state
        .releases
        .resolve_active_site(state.database.as_ref(), &project_slug)
        .await
    {
        Ok(Some(release)) => release,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return release_error_response(error),
    };

    match resolve_site_asset(FilePath::new(&release.root_dir), request_path) {
        Ok(Some(asset)) => asset.into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_error) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
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

#[derive(Debug)]
struct AssetResponse {
    bytes: Vec<u8>,
    content_type: String,
}

impl IntoResponse for AssetResponse {
    fn into_response(self) -> Response {
        let mut response = Response::new(axum::body::Body::from(self.bytes));
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_str(&self.content_type).unwrap(),
        );

        response
    }
}

fn resolve_site_asset(
    root_dir: &FilePath,
    request_path: &str,
) -> std::io::Result<Option<AssetResponse>> {
    let requested_path = match normalize_requested_asset_path(request_path) {
        Some(path) => path,
        None => return Ok(None),
    };

    if let Some(asset) = load_site_asset(root_dir, &requested_path)? {
        return Ok(Some(asset));
    }

    if requested_path != "index.html" && should_use_spa_fallback(request_path) {
        if let Some(asset) = load_site_asset(root_dir, "index.html")? {
            return Ok(Some(asset));
        }
    }

    Ok(None)
}

fn load_site_asset(
    root_dir: &FilePath,
    requested_path: &str,
) -> std::io::Result<Option<AssetResponse>> {
    let asset_path = root_dir.join(requested_path);

    if !asset_path.is_file() {
        return Ok(None);
    }

    let bytes = fs::read(&asset_path)?;

    Ok(Some(AssetResponse {
        bytes,
        content_type: mime_guess::from_path(requested_path)
            .first_or_octet_stream()
            .to_string(),
    }))
}

fn normalize_requested_asset_path(request_path: &str) -> Option<String> {
    let trimmed_path = request_path.trim_start_matches('/');

    if trimmed_path.is_empty() {
        return Some("index.html".to_owned());
    }

    let mut parts = Vec::new();

    for component in FilePath::new(trimmed_path).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => return None,
        }
    }

    if parts.is_empty() {
        return Some("index.html".to_owned());
    }

    let joined = parts.join("/");

    if request_path.ends_with('/') {
        Some(format!("{joined}/index.html"))
    } else {
        Some(joined)
    }
}

fn should_use_spa_fallback(request_path: &str) -> bool {
    let path = request_path.trim_end_matches('/');

    if path.is_empty() {
        return false;
    }

    let last_segment = path.rsplit('/').next().unwrap_or_default();

    !last_segment.contains('.')
}
