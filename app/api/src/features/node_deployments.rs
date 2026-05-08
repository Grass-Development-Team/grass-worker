use crate::AppState;
use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::{Path, rejection::JsonRejection},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{post, put},
};
use grass_worker_database::entities::{deployment, deployment_artifact};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct NodeDeploymentRouteState {
    app: AppState,
    shared_token: String,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Serialize)]
struct ClaimedDeploymentResponse {
    id: Uuid,
    project_id: Uuid,
    status: &'static str,
    source_branch: String,
    source_revision: Option<String>,
    last_stage: Option<String>,
    failure_message: Option<String>,
    repository_url: String,
    production_branch: String,
    root_directory: Option<String>,
    install_command: String,
    build_command: String,
    output_directory: String,
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<crate::domain::deployment::ClaimedGitDeployment> for ClaimedDeploymentResponse {
    fn from(value: crate::domain::deployment::ClaimedGitDeployment) -> Self {
        Self {
            id: value.id,
            project_id: value.project_id,
            status: deployment_status_label(value.status),
            source_branch: value.source_branch,
            source_revision: value.source_revision,
            last_stage: value.last_stage,
            failure_message: value.failure_message,
            repository_url: value.repository_url,
            production_branch: value.production_branch,
            root_directory: value.root_directory,
            install_command: value.install_command,
            build_command: value.build_command,
            output_directory: value.output_directory,
            started_at: value.started_at,
            finished_at: value.finished_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct ClaimedDeploymentEnvelope {
    deployment: ClaimedDeploymentResponse,
}

#[derive(Debug, Serialize)]
struct DeploymentResponse {
    id: Uuid,
    project_id: Uuid,
    status: &'static str,
    source_branch: Option<String>,
    source_revision: Option<String>,
    last_stage: Option<String>,
    failure_message: Option<String>,
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
            last_stage: value.last_stage,
            failure_message: value.failure_message,
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
struct DeploymentUploadEnvelope {
    deployment: DeploymentResponse,
    artifact: DeploymentArtifactResponse,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum NodeDeploymentStatusRequest {
    Processing,
    Ready,
    Failed,
    Canceled,
}

#[derive(Debug, Deserialize)]
struct UpdateDeploymentStageRequest {
    stage: String,
    #[serde(default)]
    status: Option<NodeDeploymentStatusRequest>,
    #[serde(default)]
    failure_message: Option<String>,
}

pub fn install_node_deployment_routes(
    router: Router,
    state: AppState,
    shared_token: String,
) -> Router {
    let internal_router = Router::new()
        .route("/api/v1/internal/deployments/claim", post(claim_deployment))
        .route(
            "/api/v1/internal/deployments/{deployment_id}/stage",
            post(update_deployment_stage),
        )
        .route(
            "/api/v1/internal/deployments/{deployment_id}/build-log",
            put(upload_build_log),
        )
        .route(
            "/api/v1/internal/deployments/{deployment_id}/static-site",
            put(upload_static_site),
        )
        .layer(Extension(NodeDeploymentRouteState {
            app: state,
            shared_token,
        }));

    router.merge(internal_router)
}

async fn claim_deployment(
    Extension(state): Extension<NodeDeploymentRouteState>,
    headers: HeaderMap,
) -> axum::response::Response {
    if let Err(response) = require_bearer_token(&headers, &state.shared_token) {
        return response;
    }

    match state
        .app
        .deployments
        .claim_next_git_backed_production_deployment(state.app.database.as_ref())
        .await
    {
        Ok(Some(deployment)) => Json(ClaimedDeploymentEnvelope {
            deployment: deployment.into(),
        })
        .into_response(),
        Ok(None) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => deployment_error_response(error),
    }
}

async fn update_deployment_stage(
    Extension(state): Extension<NodeDeploymentRouteState>,
    headers: HeaderMap,
    Path(deployment_id): Path<String>,
    payload: Result<Json<UpdateDeploymentStageRequest>, JsonRejection>,
) -> axum::response::Response {
    if let Err(response) = require_bearer_token(&headers, &state.shared_token) {
        return response;
    }

    let deployment_id = match parse_deployment_id(&deployment_id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(error = %error, "node deployment stage request rejected: invalid payload");
            return error_response(StatusCode::BAD_REQUEST, error.to_string());
        }
    };

    match state
        .app
        .deployments
        .update_stage_for_node(
            state.app.database.as_ref(),
            deployment_id,
            payload.stage.as_str(),
            deployment_status(payload.status),
            payload.failure_message.as_deref(),
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

async fn upload_build_log(
    Extension(state): Extension<NodeDeploymentRouteState>,
    headers: HeaderMap,
    Path(deployment_id): Path<String>,
    body: Bytes,
) -> axum::response::Response {
    if let Err(response) = require_bearer_token(&headers, &state.shared_token) {
        return response;
    }
    if !content_type_is_text_plain(&headers) {
        return error_response(StatusCode::BAD_REQUEST, "content type must be text/plain");
    }

    let deployment_id = match parse_deployment_id(&deployment_id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    let deployment = match state
        .app
        .deployments
        .get_for_node(state.app.database.as_ref(), deployment_id)
        .await
    {
        Ok(deployment) => deployment,
        Err(error) => return deployment_error_response(error),
    };

    let stored = match store_build_log(deployment.project_id, deployment.id, &body) {
        Ok(stored) => stored,
        Err(error) => {
            tracing::error!(error = %error, "store build log failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal build log storage error",
            );
        }
    };

    match state
        .app
        .deployments
        .register_artifact_for_loaded_deployment(
            state.app.database.as_ref(),
            &deployment,
            crate::domain::deployment::RegisterDeploymentArtifactInput {
                kind: deployment_artifact::ArtifactKind::BuildLog,
                storage_path: stored.path.to_string_lossy().into_owned(),
                checksum_sha256: Some(stored.checksum_sha256),
                size_bytes: Some(stored.size_bytes),
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

async fn upload_static_site(
    Extension(state): Extension<NodeDeploymentRouteState>,
    headers: HeaderMap,
    Path(deployment_id): Path<String>,
    body: Bytes,
) -> axum::response::Response {
    if let Err(response) = require_bearer_token(&headers, &state.shared_token) {
        return response;
    }
    if !content_type_is_application_zip(&headers) {
        return error_response(StatusCode::BAD_REQUEST, "content type must be application/zip");
    }

    let deployment_id = match parse_deployment_id(&deployment_id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    let deployment = match state
        .app
        .deployments
        .get_for_node(state.app.database.as_ref(), deployment_id)
        .await
    {
        Ok(deployment) => deployment,
        Err(error) => return deployment_error_response(error),
    };

    let file_name = bundle_file_name(&headers).unwrap_or("site.zip");
    let stored = match crate::adapters::static_site_storage::store_uploaded_zip(
        deployment.project_id,
        deployment.id,
        Some(file_name),
        &body,
    ) {
        Ok(stored) => stored,
        Err(crate::adapters::static_site_storage::StaticSiteStorageError::InvalidArchive(
            message,
        )) => return error_response(StatusCode::BAD_REQUEST, message),
        Err(error) => {
            tracing::error!(error = %error, "store node static site bundle failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal static site storage error",
            );
        }
    };

    let system_actor = crate::domain::auth::AuthenticatedUser {
        id: Uuid::nil(),
        email: "node@system.local".to_owned(),
        is_admin: true,
        is_initial_admin: false,
    };

    match state
        .app
        .deployments
        .store_static_site_artifact_for_project(
            state.app.database.as_ref(),
            &system_actor,
            deployment.project_id,
            deployment.id,
            stored.root_dir.to_string_lossy().into_owned(),
            Some(stored.checksum_sha256),
            Some(stored.size_bytes),
        )
        .await
    {
        Ok((deployment, artifact)) => (
            StatusCode::CREATED,
            Json(DeploymentUploadEnvelope {
                deployment: deployment.into(),
                artifact: artifact.into(),
            }),
        )
            .into_response(),
        Err(error) => deployment_error_response(error),
    }
}

fn require_bearer_token(
    headers: &HeaderMap,
    shared_token: &str,
) -> Result<(), axum::response::Response> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Err(error_response(StatusCode::UNAUTHORIZED, "unauthorized"));
    };
    let Ok(value) = value.to_str() else {
        return Err(error_response(StatusCode::UNAUTHORIZED, "unauthorized"));
    };
    let Some(token) = value.strip_prefix("Bearer ") else {
        return Err(error_response(StatusCode::UNAUTHORIZED, "unauthorized"));
    };
    if token != shared_token {
        return Err(error_response(StatusCode::UNAUTHORIZED, "unauthorized"));
    }

    Ok(())
}

fn content_type_is_text_plain(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_ascii_lowercase().starts_with("text/plain"))
        .unwrap_or(false)
}

fn content_type_is_application_zip(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_ascii_lowercase().starts_with("application/zip"))
        .unwrap_or(false)
}

fn bundle_file_name<'a>(headers: &'a HeaderMap) -> Option<&'a str> {
    let value = headers.get(header::CONTENT_DISPOSITION)?.to_str().ok()?;
    value
        .split(';')
        .map(str::trim)
        .find_map(|segment| segment.strip_prefix("filename=\""))
        .and_then(|value| value.strip_suffix('"'))
}

fn parse_deployment_id(raw: &str) -> Result<Uuid, &'static str> {
    Uuid::parse_str(raw).map_err(|_error| "invalid deployment id")
}

fn deployment_status(status: Option<NodeDeploymentStatusRequest>) -> deployment::DeploymentStatus {
    match status.unwrap_or(NodeDeploymentStatusRequest::Processing) {
        NodeDeploymentStatusRequest::Processing => deployment::DeploymentStatus::Processing,
        NodeDeploymentStatusRequest::Ready => deployment::DeploymentStatus::Ready,
        NodeDeploymentStatusRequest::Failed => deployment::DeploymentStatus::Failed,
        NodeDeploymentStatusRequest::Canceled => deployment::DeploymentStatus::Canceled,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredBuildLog {
    path: PathBuf,
    checksum_sha256: String,
    size_bytes: i64,
}

fn artifact_root() -> PathBuf {
    std::env::var_os("GRASS_WORKER_ARTIFACT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("grass-worker-artifacts"))
}

fn store_build_log(
    project_id: Uuid,
    deployment_id: Uuid,
    bytes: &[u8],
) -> Result<StoredBuildLog, std::io::Error> {
    let checksum_sha256 = hex::encode(Sha256::digest(bytes));
    let size_bytes = i64::try_from(bytes.len())
        .map_err(|_error| std::io::Error::other("build log is too large to store"))?;
    let root = artifact_root()
        .join(project_id.to_string())
        .join(deployment_id.to_string())
        .join("build-logs");
    std::fs::create_dir_all(&root)?;
    let path = root.join(format!("{checksum_sha256}.log"));
    std::fs::write(&path, bytes)?;

    Ok(StoredBuildLog {
        path,
        checksum_sha256,
        size_bytes,
    })
}
