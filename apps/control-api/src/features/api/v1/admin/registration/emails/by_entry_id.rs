use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::EntityTrait;
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, registration_email_allowlist},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/registration/emails/{entry_id}",
        axum::routing::delete(remove),
    )
}

async fn remove(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(entry_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.registration.emails.remove";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;
    let entry = registration_email_allowlist::Entity::find_by_id(entry_id)
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "registration email was not found".to_owned(),
        })?;
    registration_email_allowlist::Entity::delete_by_id(entry.id)
        .exec(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "registration.email_removed".to_owned(),
            target_type: "registration_email".to_owned(),
            target_id: Some(entry.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "email": entry.email }),
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

    Ok(ok_response(RemoveResponse { deleted: true }))
}

#[derive(serde::Serialize)]
struct RemoveResponse {
    deleted: bool,
}
