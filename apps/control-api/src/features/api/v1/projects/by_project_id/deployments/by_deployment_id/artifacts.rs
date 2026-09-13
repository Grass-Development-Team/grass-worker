use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use uuid::Uuid;

use crate::{
    domain::deployments,
    infra::{
        database::entity::deployment,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

#[derive(serde::Serialize)]
struct ArtifactsResponse {
    artifacts: Vec<ArtifactResponse>,
}

#[derive(serde::Serialize)]
struct ArtifactResponse {
    id: Uuid,
    kind: &'static str,
    storage_path: String,
    checksum_sha256: Option<String>,
    size_bytes: Option<i64>,
    manifest: serde_json::Value,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/deployments/{deployment_id}/artifacts",
        axum::routing::get(artifacts),
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

fn artifact_kind_value(
    kind: &crate::infra::database::entity::DeploymentArtifactKind,
) -> &'static str {
    use crate::infra::database::entity::DeploymentArtifactKind as K;
    match kind {
        K::GrassOutput => "grass_output",
        K::BuildLog => "build_log",
        K::StaticSite => "static_site",
        K::Screenshot => "screenshot",
    }
}

/// GET /api/v1/projects/{project_id}/deployments/{deployment_id}/artifacts
async fn artifacts(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.artifacts";
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

    let artifacts = deployments::list_artifacts(db, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(ArtifactsResponse {
        artifacts: artifacts
            .iter()
            .map(|artifact| ArtifactResponse {
                id: artifact.id,
                kind: artifact_kind_value(&artifact.kind),
                storage_path: artifact.storage_path.clone(),
                checksum_sha256: artifact.checksum_sha256.clone(),
                size_bytes: artifact.size_bytes,
                manifest: artifact.manifest.clone(),
                created_at: artifact.created_at,
            })
            .collect::<Vec<_>>(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::database::entity::DeploymentArtifactKind;

    #[test]
    fn screenshot_artifacts_use_the_public_api_kind() {
        assert_eq!(
            artifact_kind_value(&DeploymentArtifactKind::Screenshot),
            "screenshot"
        );
    }
}
