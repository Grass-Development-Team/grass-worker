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
        database::entity::{AuditEventResult, announcement},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/announcements/{announcement_id}",
        axum::routing::delete(remove),
    )
}

fn database<'a>(
    state: &'a ControlApiState,
    op: &'static str,
) -> Result<&'a sea_orm::DatabaseConnection, AppError> {
    crate::infra::http::database(state, op)
}

async fn remove(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(announcement_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.announcements.delete";
    let db = database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let announcement = announcement::Entity::find_by_id(announcement_id)
        .one(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "announcement not found".to_owned(),
        })?;

    announcement::Entity::delete_by_id(announcement_id)
        .exec(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    audits::create_platform_audit_event_with_changes(
        &transaction,
        audits::CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "site.announcement_deleted".to_owned(),
            target_type: "announcement".to_owned(),
            target_id: Some(announcement.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({}),
        },
        json!({
            "before": {
                "title": announcement.title,
                "content": announcement.content,
                "auto_popup": announcement.auto_popup,
            },
            "after": null,
        }),
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

    Ok(ok_response(RemoveResponse { ok: true }))
}

#[derive(serde::Serialize)]
struct RemoveResponse {
    ok: bool,
}
