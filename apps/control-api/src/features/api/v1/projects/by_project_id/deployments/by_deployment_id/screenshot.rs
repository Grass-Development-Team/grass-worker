use axum::{
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::Response,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::{
    domain::deployments,
    infra::{
        database::entity::{
            DeploymentArtifactKind, deployment, deployment_artifact, deployment_screenshot_job,
        },
        error::AppError,
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/deployments/{deployment_id}/screenshot",
        axum::routing::get(screenshot),
    )
}

async fn load_deployment(
    db: &sea_orm::DatabaseConnection,
    access: &crate::domain::project_access::ProjectAccess,
    deployment_id: Uuid,
    op: &'static str,
) -> Result<deployment::Model, AppError> {
    let deployment = deployments::get_by_id(db, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "deployment not found".to_owned(),
        })?;
    if deployment.project_id != access.project.id {
        return Err(AppError::NotFound {
            op,
            message: "deployment not found".to_owned(),
        });
    }
    Ok(deployment)
}

/// GET /api/v1/projects/{project_id}/deployments/{deployment_id}/screenshot
pub async fn screenshot(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, AppError> {
    const OP: &str = "deployments.screenshot";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    let db = crate::infra::http::database(&state, OP)?;
    let deployment = load_deployment(db, &access, deployment_id, OP).await?;
    let artifact_id = deployment_screenshot_job::Entity::find_by_id(deployment.id)
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .and_then(|job| {
            matches!(
                job.status,
                crate::infra::database::entity::DeploymentScreenshotStatus::Succeeded
            )
            .then_some(job.artifact_id)
            .flatten()
        })
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment screenshot not found".to_owned(),
        })?;
    let artifact = deployment_artifact::Entity::find_by_id(artifact_id)
        .filter(deployment_artifact::Column::DeploymentId.eq(deployment.id))
        .filter(deployment_artifact::Column::Kind.eq(DeploymentArtifactKind::Screenshot))
        .filter(deployment_artifact::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment screenshot not found".to_owned(),
        })?;
    let storage = state.storage.clone();
    let object = storage
        .open_artifact(&artifact.storage_path)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment screenshot not found".to_owned(),
        })?;
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/webp")
        .header(header::CONTENT_LENGTH, object.size_bytes)
        .header(header::CACHE_CONTROL, "private, max-age=3600");
    if let Some(checksum) = artifact.checksum_sha256 {
        builder = builder.header(header::ETAG, format!("\"{checksum}\""));
    }
    builder
        .body(Body::from_stream(object.stream))
        .map_err(|error| AppError::Internal {
            op: OP,
            message: format!("failed to build screenshot response: {error}"),
        })
}
