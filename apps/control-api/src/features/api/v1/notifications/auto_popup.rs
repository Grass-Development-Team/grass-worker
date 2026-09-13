use axum::{extract::State, response::IntoResponse};

use crate::{
    domain::notifications,
    infra::{
        database::entity::user_notification,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/notifications/auto-popup", axum::routing::get(auto_popup))
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

#[derive(serde::Serialize)]
struct NotificationResponse {
    id: uuid::Uuid,
    action: String,
    announcement_id: Option<uuid::Uuid>,
    title: String,
    project: Option<NotificationProjectResponse>,
    content: Option<String>,
    reason: Option<String>,
    target_url: String,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    read_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}
#[derive(serde::Serialize)]
struct NotificationProjectResponse {
    id: Option<uuid::Uuid>,
    name: Option<String>,
    slug: Option<String>,
}
fn notification_view(item: &user_notification::Model) -> NotificationResponse {
    NotificationResponse {
        id: item.id,
        action: item.action.clone(),
        announcement_id: item.announcement_id,
        title: item
            .title
            .as_deref()
            .unwrap_or_else(|| notifications::notification_title(&item.action))
            .to_owned(),
        project: (item.action != "site.announcement").then(|| NotificationProjectResponse {
            id: item.project_id,
            name: item.project_name.clone(),
            slug: item.project_slug.clone(),
        }),
        content: item.content.clone(),
        reason: item.reason.clone(),
        target_url: item.target_url.clone(),
        read_at: item.read_at,
        created_at: item.created_at,
    }
}

async fn auto_popup(
    State(state): State<ControlApiState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "notifications.auto_popup";
    let notification =
        notifications::latest_auto_popup(database(&state, OP)?, session.data.user_id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(AutoPopupResponse {
        notification: notification.as_ref().map(notification_view),
    }))
}

#[derive(serde::Serialize)]
struct AutoPopupResponse {
    notification: Option<NotificationResponse>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

    use crate::infra::database::entity::user_notification;
    #[test]
    fn notification_response_hides_actor_and_contains_project_reason_and_target() {
        let item = user_notification::Model {
            id: uuid::Uuid::now_v7(),
            recipient_user_id: uuid::Uuid::now_v7(),
            actor_user_id: Some(uuid::Uuid::now_v7()),
            team_id: Some(uuid::Uuid::now_v7()),
            project_id: Some(uuid::Uuid::now_v7()),
            announcement_id: None,
            action: "project.slug_updated".to_owned(),
            project_name: Some("Demo".to_owned()),
            project_slug: Some("demo-site".to_owned()),
            actor_label: "Platform Admin".to_owned(),
            title: None,
            content: None,
            reason: Some("Reserved wording".to_owned()),
            target_url: "/projects/demo/deployments".to_owned(),
            read_at: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
        };

        let value = serde_json::to_value(notification_view(&item)).unwrap();

        assert_eq!(value["title"], "Project slug changed");
        assert_eq!(value["project"]["name"], "Demo");
        assert_eq!(value["project"]["slug"], "demo-site");
        assert!(value.get("actor").is_none());
        assert_eq!(value["reason"], "Reserved wording");
        assert_eq!(value["target_url"], "/projects/demo/deployments");
        assert!(value["read_at"].is_null());
    }

    #[test]
    fn announcement_response_exposes_content_without_project_metadata() {
        let item = user_notification::Model {
            id: uuid::Uuid::now_v7(),
            recipient_user_id: uuid::Uuid::now_v7(),
            actor_user_id: Some(uuid::Uuid::now_v7()),
            team_id: None,
            project_id: None,
            announcement_id: None,
            action: "site.announcement".to_owned(),
            project_name: None,
            project_slug: None,
            actor_label: "Platform Admin".to_owned(),
            title: Some("Maintenance window".to_owned()),
            content: Some("The API will restart at 10:00 UTC.".to_owned()),
            reason: None,
            target_url: "/notifications".to_owned(),
            read_at: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
        };

        let value = serde_json::to_value(notification_view(&item)).unwrap();

        assert_eq!(value["title"], "Maintenance window");
        assert_eq!(value["content"], "The API will restart at 10:00 UTC.");
        assert!(value["project"].is_null());
        assert_eq!(value["target_url"], "/notifications");
    }
}
