pub(crate) mod auto_popup;
pub(crate) mod by_notification_id;
pub(crate) mod read_all;
pub(crate) mod unread_count;

use axum::{
    extract::{Query, State},
    response::IntoResponse,
};
use serde::Deserialize;

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
    axum::Router::new()
        .route("/notifications", axum::routing::get(list))
        .merge(auto_popup::router())
        .merge(by_notification_id::router())
        .merge(read_all::router())
        .merge(unread_count::router())
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

    Ok(ok_response(ListResponse {
        notifications: page
            .notifications
            .iter()
            .map(notification_view)
            .collect::<Vec<_>>(),
        pagination: ListPaginationResponse {
            page: page.page,
            per_page: page.per_page,
            total: page.total,
            total_pages: page.total_pages,
        },
    }))
}

#[derive(serde::Serialize)]
struct ListPaginationResponse {
    page: u64,
    per_page: u64,
    total: u64,
    total_pages: u64,
}

#[derive(serde::Serialize)]
struct ListResponse {
    notifications: Vec<NotificationResponse>,
    pagination: ListPaginationResponse,
}
