use axum::{
    Router,
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::notifications,
    infra::{
        database::entity::user_notification,
        error::{AppError, ok_response},
        http::{extractors::Session, timestamps::ts},
    },
    state::ControlApiState,
};

pub fn router() -> Router<ControlApiState> {
    Router::new()
        .route("/notifications", get(list))
        .route("/notifications/unread-count", get(unread_count))
        .route("/notifications/read-all", post(mark_all_read))
        .route("/notifications/{notification_id}/read", post(mark_read))
}

#[derive(Default, Deserialize)]
pub struct ListQuery {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

fn database<'a>(
    state: &'a ControlApiState,
    op: &'static str,
) -> Result<&'a sea_orm::DatabaseConnection, AppError> {
    state.try_database().ok_or_else(|| AppError::Internal {
        op,
        message: "database not available".to_owned(),
    })
}

fn notification_view(item: &user_notification::Model) -> serde_json::Value {
    json!({
        "id": item.id,
        "action": item.action,
        "title": notifications::notification_title(&item.action),
        "project": {
            "id": item.project_id,
            "name": item.project_name,
            "slug": item.project_slug,
        },
        "actor": {
            "id": item.actor_user_id,
            "label": item.actor_label,
        },
        "reason": item.reason,
        "target_url": item.target_url,
        "read_at": ts(item.read_at),
        "created_at": ts(item.created_at),
    })
}

pub async fn list(
    State(state): State<ControlApiState>,
    session: Session,
    Query(query): Query<ListQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "notifications.list";
    let page = notifications::list_for_user(
        database(&state, OP)?,
        session.data.user_id,
        query.page.unwrap_or(1),
        query.per_page.unwrap_or(25),
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(json!({
        "notifications": page.notifications.iter().map(notification_view).collect::<Vec<_>>(),
        "pagination": {
            "page": page.page,
            "per_page": page.per_page,
            "total": page.total,
            "total_pages": page.total_pages,
        },
    })))
}

pub async fn unread_count(
    State(state): State<ControlApiState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "notifications.unread_count";
    let count = notifications::unread_count(database(&state, OP)?, session.data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(json!({ "count": count })))
}

pub async fn mark_read(
    State(state): State<ControlApiState>,
    session: Session,
    Path(notification_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "notifications.mark_read";
    let found =
        notifications::mark_read(database(&state, OP)?, session.data.user_id, notification_id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    if !found {
        return Err(AppError::NotFound {
            op: OP,
            message: "notification not found".to_owned(),
        });
    }
    Ok(ok_response(json!({ "ok": true })))
}

pub async fn mark_all_read(
    State(state): State<ControlApiState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "notifications.mark_all_read";
    let updated = notifications::mark_all_read(database(&state, OP)?, session.data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(json!({ "updated": updated })))
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use super::*;

    #[test]
    fn notification_response_contains_actor_project_reason_and_target() {
        let item = user_notification::Model {
            id: Uuid::now_v7(),
            recipient_user_id: Uuid::now_v7(),
            actor_user_id: Some(Uuid::now_v7()),
            team_id: Some(Uuid::now_v7()),
            project_id: Some(Uuid::now_v7()),
            action: "project.slug_updated".to_owned(),
            project_name: "Demo".to_owned(),
            project_slug: "demo-site".to_owned(),
            actor_label: "Platform Admin".to_owned(),
            reason: Some("Reserved wording".to_owned()),
            target_url: "/projects/demo/deployments".to_owned(),
            read_at: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
        };

        let value = notification_view(&item);

        assert_eq!(value["title"], "Project slug changed");
        assert_eq!(value["project"]["name"], "Demo");
        assert_eq!(value["project"]["slug"], "demo-site");
        assert_eq!(value["actor"]["label"], "Platform Admin");
        assert_eq!(value["reason"], "Reserved wording");
        assert_eq!(value["target_url"], "/projects/demo/deployments");
        assert!(value["read_at"].is_null());
    }
}
