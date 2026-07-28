use axum::{
    Extension,
    extract::{Query, State},
    response::IntoResponse,
};
use grass_node_protocol::{ResolveHostResponse, ServeAccess};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{deployments, hosts},
    infra::{
        database::entity::{
            DeploymentArtifactKind, DeploymentBuildStatus, DeploymentEnvironment,
            HostBindingEnvironment, HostBindingStatus, deployment_artifact,
        },
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct ResolveHostQuery {
    pub host: String,
}

async fn artifact_available(
    db: &sea_orm::DatabaseConnection,
    deployment_id: Uuid,
) -> anyhow::Result<bool> {
    Ok(deployment_artifact::Entity::find()
        .filter(deployment_artifact::Column::DeploymentId.eq(deployment_id))
        .filter(deployment_artifact::Column::Kind.eq(DeploymentArtifactKind::GrassOutput))
        .one(db)
        .await?
        .is_some())
}

/// GET /api/v1/internal/serve/resolve-host?host=...
///
/// Maps a public Host header to the deployment that should serve it:
/// production bindings resolve to the active production deployment only;
/// preview hosts resolve to their ready preview deployment.
pub async fn resolve_host(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(_node)): Extension<AuthenticatedNode>,
    Query(query): Query<ResolveHostQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.serve.resolve_host";
    let db = super::database(&state, OP)?;

    let host =
        grass_validator::normalize_host(&query.host).map_err(|error| AppError::Validation {
            op: OP,
            message: error.to_string(),
        })?;

    // Production bindings first: stable domains only ever serve the active
    // production deployment.
    if let Some(binding) = hosts::find_binding_by_host(db, &host)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
    {
        if !matches!(binding.status, HostBindingStatus::Active) {
            return Err(AppError::NotFound {
                op: OP,
                message: "host binding is not active".to_owned(),
            });
        }
        if matches!(binding.environment, HostBindingEnvironment::Preview) {
            return Err(AppError::NotFound {
                op: OP,
                message: "preview-only bindings resolve through deployment hosts".to_owned(),
            });
        }

        let active =
            deployments::find_active(db, binding.project_id, DeploymentEnvironment::Production)
                .await
                .map_err(|source| AppError::Infrastructure { op: OP, source })?
                .ok_or_else(|| AppError::NotFound {
                    op: OP,
                    message: "no active production deployment for this host".to_owned(),
                })?;

        let available = artifact_available(db, active.id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        return Ok(ok_response(ResolveHostResponse {
            deployment_id: active.id,
            project_id: active.project_id,
            team_id: active.team_id,
            host,
            environment: "production".to_owned(),
            artifact_available: available,
            access: ServeAccess::Public,
        }));
    }

    // Preview hosts are stored directly on the deployment row.
    let deployment = deployments::find_by_preview_host(db, &host)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "host is not bound".to_owned(),
        })?;

    if !matches!(deployment.build_status, DeploymentBuildStatus::Ready) {
        return Err(AppError::NotFound {
            op: OP,
            message: "preview deployment is not ready".to_owned(),
        });
    }

    let available = artifact_available(db, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(ResolveHostResponse {
        deployment_id: deployment.id,
        project_id: deployment.project_id,
        team_id: deployment.team_id,
        host,
        environment: "preview".to_owned(),
        artifact_available: available,
        access: ServeAccess::TeamMember,
    }))
}
