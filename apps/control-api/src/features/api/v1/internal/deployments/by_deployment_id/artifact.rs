use axum::{
    Extension,
    body::Body,
    extract::{Path, State},
    http::{HeaderValue, header},
    response::Response,
};
use grass_node_protocol::artifact_headers;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::{
    domain::deployments,
    infra::{
        database::entity::{
            DeploymentArtifactKind, deployment_artifact, node_deployment_migration,
        },
        error::AppError,
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route(
        "/deployments/{deployment_id}/artifact",
        axum::routing::get(download_artifact),
    )
}

/// GET /api/v1/internal/deployments/{deployment_id}/artifact
///
/// Lets a serve Node re-fetch the grass-output archive after cache loss.
pub async fn download_artifact(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
) -> Result<Response, AppError> {
    const OP: &str = "internal.deployments.download_artifact";
    let db = crate::infra::http::database(&state, OP)?;

    let deployment = deployments::get_by_id(db, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment not found".to_owned(),
        })?;
    let is_migration_target = node_deployment_migration::Entity::find()
        .filter(node_deployment_migration::Column::DeploymentId.eq(deployment.id))
        .filter(node_deployment_migration::Column::TargetNodeId.eq(node.id))
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .is_some_and(|migration| {
            crate::domain::node_migration_access::migration_allows_artifact_download(
                &migration.status,
            )
        });
    if deployment.serve_node_id != Some(node.id) && !is_migration_target {
        return Err(AppError::Forbidden {
            op: OP,
            message: "deployment is not assigned to this Serve Node".to_owned(),
        });
    }
    let artifact = deployment_artifact::Entity::find()
        .filter(deployment_artifact::Column::DeploymentId.eq(deployment.id))
        .filter(deployment_artifact::Column::Kind.eq(DeploymentArtifactKind::GrassOutput))
        .filter(deployment_artifact::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "artifact not found".to_owned(),
        })?;
    let opened = state
        .storage
        .clone()
        .open_artifact(&artifact.storage_path)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "artifact not found".to_owned(),
        })?;
    let packed_size_bytes = artifact
        .size_bytes
        .and_then(|size| u64::try_from(size).ok())
        .ok_or_else(|| AppError::Internal {
            op: OP,
            message: "artifact packed size metadata is missing".to_owned(),
        })?;
    if opened.size_bytes != packed_size_bytes {
        return Err(AppError::Internal {
            op: OP,
            message: "artifact file size does not match its metadata".to_owned(),
        });
    }
    let checksum_sha256 = artifact.checksum_sha256.ok_or_else(|| AppError::Internal {
        op: OP,
        message: "artifact checksum metadata is missing".to_owned(),
    })?;
    let unpacked_size_bytes = deployments::artifact_unpacked_size_bytes(&artifact.manifest)
        .ok_or_else(|| AppError::Internal {
            op: OP,
            message: "artifact unpacked size metadata is invalid".to_owned(),
        })?;
    let mut response = Response::new(Body::from_stream(opened.stream));
    let response_headers = response.headers_mut();
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    for (name, value) in [
        (
            header::CONTENT_LENGTH.as_str(),
            packed_size_bytes.to_string(),
        ),
        (
            artifact_headers::PACKED_SIZE_BYTES,
            packed_size_bytes.to_string(),
        ),
        (
            artifact_headers::UNPACKED_SIZE_BYTES,
            unpacked_size_bytes.to_string(),
        ),
        (artifact_headers::CHECKSUM_SHA256, checksum_sha256),
    ] {
        response_headers.insert(
            axum::http::HeaderName::from_static(name),
            HeaderValue::from_str(&value).map_err(|_| AppError::Internal {
                op: OP,
                message: "artifact response metadata is invalid".to_owned(),
            })?,
        );
    }
    Ok(response)
}
