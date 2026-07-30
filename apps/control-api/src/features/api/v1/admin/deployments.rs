use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::TransactionTrait;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        delivery::{self, PublicationRemovalKind},
        deployments,
    },
    infra::{
        database::entity::{AuditEventResult, deployment},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

#[derive(Default, Deserialize)]
pub struct GovernanceReason {
    #[serde(default)]
    pub reason: Option<String>,
}

fn optional_reason(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    })
}

/// POST /api/v1/admin/deployments/{deployment_id}/withdraw
///
/// Immediately removes every route for the deployment, preserves its stored
/// data, and invalidates the previous review by returning it to draft.
pub async fn withdraw(
    State(state): State<ControlApiState>,
    session: Session,
    Path(deployment_id): Path<Uuid>,
    body: Option<Json<GovernanceReason>>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.deployments.withdraw";
    let db = super::database(&state, OP)?;
    let transaction = db
        .begin()
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
    let reason = optional_reason(body.and_then(|Json(body)| body.reason));
    let deployment = delivery::remove_publication(
        &transaction,
        deployment,
        PublicationRemovalKind::PlatformAdmin,
    )
    .await
    .map_err(|error| {
        crate::features::api::v1::projects::deployments::map_delivery_error(error, OP)
    })?;
    audits::create_platform_audit_event(
        &transaction,
        CreateAuditEventParams {
            actor_user_id: Some(session.data.user_id),
            actor_node_id: None,
            team_id: Some(deployment.team_id),
            action: "deployment.withdrawn".to_owned(),
            target_type: "deployment".to_owned(),
            target_id: Some(deployment.id),
            result: AuditEventResult::Success,
            reason: reason.clone(),
            metadata: json!({
                "platform_admin": true,
                "project_id": deployment.project_id,
            }),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(json!({
        "deployment": deployment_view(&deployment),
        "reason": reason,
    })))
}

fn deployment_view(deployment: &deployment::Model) -> serde_json::Value {
    json!({
        "id": deployment.id,
        "project_id": deployment.project_id,
        "release_status": deployments::release_status_value(&deployment.release_status),
        "serve_status": deployments::serve_status_value(&deployment.serve_status),
    })
}

#[cfg(test)]
mod tests {
    use super::optional_reason;

    #[test]
    fn administrator_reason_is_optional_and_normalized() {
        assert_eq!(optional_reason(None), None);
        assert_eq!(optional_reason(Some("   ".to_owned())), None);
        assert_eq!(
            optional_reason(Some("  policy violation  ".to_owned())),
            Some("policy violation".to_owned()),
        );
    }
}
