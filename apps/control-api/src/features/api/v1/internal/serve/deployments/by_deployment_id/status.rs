use axum::{
    Extension, Json,
    extract::{Path, State},
    response::IntoResponse,
};
use grass_node_protocol::ReportedServeStatus;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::{deployments, node_migration_access::shadow_migration_statuses, scheduler},
    infra::{
        database::entity::{
            DeploymentBuildStatus, DeploymentServeStatus, NodeDeploymentMigrationStatus,
            node_deployment_migration,
        },
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReportServeStatusRequest {
    pub status: ReportedServeStatus,
    #[serde(default)]
    pub failure_code: Option<String>,
    #[serde(default)]
    pub failure_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReportServeStatusResponse {
    pub acknowledged: bool,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route(
        "/serve/deployments/{deployment_id}/status",
        axum::routing::post(report_status),
    )
}

fn validate_status_report(report: &ReportServeStatusRequest) -> Result<(), &'static str> {
    if let Some(message) = &report.failure_message
        && message.len() > 1024
    {
        return Err("failure_message must be at most 1024 bytes");
    }
    if !matches!(report.status, ReportedServeStatus::Failed) {
        if report.failure_code.is_some() || report.failure_message.is_some() {
            return Err("failure details are only allowed for failed status");
        }
        return Ok(());
    }

    let Some(code) = report.failure_code.as_deref() else {
        return Err("failed status requires failure_code");
    };
    if code.is_empty()
        || code.len() > 64
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte))
    {
        return Err("failure_code must be a lowercase identifier of at most 64 bytes");
    }
    Ok(())
}

fn ensure_serve_node(
    node: &crate::infra::database::entity::node::Model,
    op: &'static str,
) -> Result<(), AppError> {
    if !node.serve_enabled {
        return Err(AppError::Forbidden {
            op,
            message: "node does not have Serve capability".to_owned(),
        });
    }
    Ok(())
}

/// POST /api/v1/internal/serve/deployments/{deployment_id}/status
pub async fn report_status(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
    Json(report): Json<ReportServeStatusRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.serve.report_status";
    ensure_serve_node(&node, OP)?;
    validate_status_report(&report).map_err(|message| AppError::Validation {
        op: OP,
        message: message.to_owned(),
    })?;
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    scheduler::lock_placement(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let deployment = deployments::get_by_id_for_update(&transaction, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment not found".to_owned(),
        })?;
    let migration = node_deployment_migration::Entity::find()
        .filter(node_deployment_migration::Column::DeploymentId.eq(deployment_id))
        .filter(node_deployment_migration::Column::TargetNodeId.eq(node.id))
        .filter(node_deployment_migration::Column::Status.is_in(shadow_migration_statuses()))
        .lock_exclusive()
        .one(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    if let Some(migration) = migration.filter(|_| deployment.serve_node_id != Some(node.id)) {
        let now = time::OffsetDateTime::now_utc();
        let (status, error, ready_at) = match report.status {
            ReportedServeStatus::Syncing => (NodeDeploymentMigrationStatus::Syncing, None, None),
            ReportedServeStatus::Ready => (NodeDeploymentMigrationStatus::Ready, None, Some(now)),
            ReportedServeStatus::Failed => (
                NodeDeploymentMigrationStatus::Failed,
                report.failure_message.or(report.failure_code),
                None,
            ),
        };
        let mut active: node_deployment_migration::ActiveModel = migration.into();
        active.status = sea_orm::ActiveValue::Set(status);
        active.error = sea_orm::ActiveValue::Set(error);
        active.ready_at = sea_orm::ActiveValue::Set(ready_at);
        active.updated_at = sea_orm::ActiveValue::Set(now);
        active
            .update(&transaction)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
        transaction
            .commit()
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
        return Ok(ok_response(ReportServeStatusResponse {
            acknowledged: true,
        }));
    }
    if deployment.serve_node_id != Some(node.id) {
        return Err(AppError::Forbidden {
            op: OP,
            message: "deployment is not assigned to this Serve Node".to_owned(),
        });
    }
    if !matches!(deployment.build_status, DeploymentBuildStatus::Ready) {
        return Err(AppError::Conflict {
            op: OP,
            message: "deployment build is not ready".to_owned(),
        });
    }

    let target = match report.status {
        ReportedServeStatus::Syncing => DeploymentServeStatus::Syncing,
        ReportedServeStatus::Ready => DeploymentServeStatus::Ready,
        ReportedServeStatus::Failed => DeploymentServeStatus::Failed,
    };
    let updated = deployments::transition_serve(
        &transaction,
        deployment,
        deployments::ServeTransition {
            to: target.clone(),
            failure_code: report.failure_code,
            failure_message: report.failure_message,
        },
    )
    .await
    .map_err(|error| crate::infra::http::deployment_errors::map_state_error(error, OP))?;
    if matches!(target, DeploymentServeStatus::Ready) {
        crate::domain::deployment_activation::complete_serve_ready(&transaction, updated).await?;
    }
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(ReportServeStatusResponse {
        acknowledged: true,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    use grass_node_protocol::ReportedServeStatus;
    #[test]
    fn serve_failure_reports_require_bounded_stable_details() {
        let valid = ReportServeStatusRequest {
            status: ReportedServeStatus::Failed,
            failure_code: Some("checksum_mismatch".to_owned()),
            failure_message: Some("downloaded artifact did not match metadata".to_owned()),
        };
        assert!(validate_status_report(&valid).is_ok());

        let mut invalid = valid.clone();
        invalid.failure_code = Some("Checksum Mismatch".to_owned());
        assert_eq!(
            validate_status_report(&invalid).unwrap_err(),
            "failure_code must be a lowercase identifier of at most 64 bytes"
        );

        invalid = valid.clone();
        invalid.failure_code = None;
        assert_eq!(
            validate_status_report(&invalid).unwrap_err(),
            "failed status requires failure_code"
        );

        invalid = valid;
        invalid.failure_message = Some("x".repeat(1025));
        assert_eq!(
            validate_status_report(&invalid).unwrap_err(),
            "failure_message must be at most 1024 bytes"
        );
    }

    #[test]
    fn non_failure_serve_reports_reject_failure_details() {
        let report = ReportServeStatusRequest {
            status: ReportedServeStatus::Ready,
            failure_code: Some("unexpected".to_owned()),
            failure_message: None,
        };

        assert_eq!(
            validate_status_report(&report).unwrap_err(),
            "failure details are only allowed for failed status"
        );
    }

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<
            ReportServeStatusRequest,
            grass_node_protocol::ReportServeStatusRequest,
        >("ReportServeStatusRequest");
        crate::test_support::assert_node_contract::<
            ReportServeStatusResponse,
            grass_node_protocol::ReportServeStatusResponse,
        >("ReportServeStatusResponse");
    }
}
