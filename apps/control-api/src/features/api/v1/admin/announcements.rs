use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, EntityTrait, PaginatorTrait, QueryOrder, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::{audits, notifications},
    infra::{
        database::entity::{AuditEventResult, announcement},
        error::{AppError, ok_response},
        http::{extractors::Session, timestamps::ts},
    },
    state::ControlApiState,
};

#[derive(Default, Deserialize)]
pub struct ListQuery {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

#[derive(Deserialize)]
pub struct PublishAnnouncementRequest {
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub auto_popup: bool,
}

#[derive(Serialize)]
struct AnnouncementView {
    id: Uuid,
    title: String,
    content: String,
    auto_popup: bool,
    published_at: serde_json::Value,
}

fn database<'a>(
    state: &'a ControlApiState,
    op: &'static str,
) -> Result<&'a sea_orm::DatabaseConnection, AppError> {
    super::database(state, op)
}

fn announcement_view(item: &announcement::Model) -> AnnouncementView {
    AnnouncementView {
        id: item.id,
        title: item.title.clone(),
        content: item.content.clone(),
        auto_popup: item.auto_popup,
        published_at: ts(item.published_at),
    }
}

fn prepare_announcement(
    body: PublishAnnouncementRequest,
    op: &'static str,
) -> Result<(String, String, bool), AppError> {
    let title = body.title.trim().to_owned();
    if title.is_empty() || title.chars().count() > 120 {
        return Err(AppError::Validation {
            op,
            message: "announcement title must contain between 1 and 120 characters".to_owned(),
        });
    }
    let content = body.content.trim().to_owned();
    if content.is_empty() || content.chars().count() > 10_000 {
        return Err(AppError::Validation {
            op,
            message: "announcement content must contain between 1 and 10000 characters".to_owned(),
        });
    }
    Ok((title, content, body.auto_popup))
}

pub async fn list(
    State(state): State<ControlApiState>,
    Query(query): Query<ListQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.announcements.list";
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(20).clamp(1, 100);
    let paginator = announcement::Entity::find()
        .order_by_desc(announcement::Column::PublishedAt)
        .order_by_desc(announcement::Column::Id)
        .paginate(database(&state, OP)?, per_page);
    let total = paginator
        .num_items()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let announcements =
        paginator
            .fetch_page(page - 1)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
    let total_pages = total.div_ceil(per_page);

    Ok(ok_response(json!({
        "announcements": announcements.iter().map(announcement_view).collect::<Vec<_>>(),
        "pagination": {
            "page": page,
            "per_page": per_page,
            "total": total,
            "total_pages": total_pages,
        },
    })))
}

pub async fn publish(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Json(body): Json<PublishAnnouncementRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.announcements.publish";
    let (title, content, auto_popup) = prepare_announcement(body, OP)?;
    let db = database(&state, OP)?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let announcement = announcement::ActiveModel {
        id: Set(Uuid::now_v7()),
        title: Set(title.clone()),
        content: Set(content.clone()),
        auto_popup: Set(auto_popup),
        created_by_user_id: Set(Some(data.user_id)),
        published_at: Set(OffsetDateTime::now_utc()),
    }
    .insert(&transaction)
    .await
    .map_err(|source| AppError::Infrastructure {
        op: OP,
        source: source.into(),
    })?;

    let recipients = notifications::create_announcement_notifications(
        &transaction,
        notifications::CreateAnnouncementNotification {
            announcement_id: announcement.id,
            actor_user_id: data.user_id,
            title: title.clone(),
            content: content.clone(),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    audits::create_platform_audit_event_with_changes(
        &transaction,
        audits::CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "site.announcement_published".to_owned(),
            target_type: "announcement".to_owned(),
            target_id: Some(announcement.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "recipients": recipients, "auto_popup": auto_popup }),
        },
        json!({
            "before": null,
            "after": {
                "title": title,
                "content": content,
                "auto_popup": auto_popup,
            },
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

    Ok(ok_response(json!({
        "announcement": announcement_view(&announcement),
        "recipients": recipients,
    })))
}

pub async fn remove(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(announcement_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.announcements.delete";
    let db = database(&state, OP)?;
    let transaction = db
        .begin()
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

    Ok(ok_response(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announcement_input_is_trimmed_and_bounded() {
        let (title, content, auto_popup) = prepare_announcement(
            PublishAnnouncementRequest {
                title: "  Planned maintenance  ".to_owned(),
                content: "  The API will restart shortly.  ".to_owned(),
                auto_popup: true,
            },
            "test.announcement",
        )
        .unwrap();
        assert_eq!(title, "Planned maintenance");
        assert_eq!(content, "The API will restart shortly.");
        assert!(auto_popup);

        assert!(
            prepare_announcement(
                PublishAnnouncementRequest {
                    title: String::new(),
                    content: "content".to_owned(),
                    auto_popup: false,
                },
                "test.announcement",
            )
            .is_err()
        );
        assert!(
            prepare_announcement(
                PublishAnnouncementRequest {
                    title: "title".to_owned(),
                    content: "x".repeat(10_001),
                    auto_popup: false,
                },
                "test.announcement",
            )
            .is_err()
        );
    }
}
