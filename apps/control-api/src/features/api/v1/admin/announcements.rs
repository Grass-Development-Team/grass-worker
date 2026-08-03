use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::TransactionTrait;
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::{audits, notifications, settings},
    infra::{
        database::entity::AuditEventResult,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

const TITLE_KEY: &str = "site.announcement.title";
const CONTENT_KEY: &str = "site.announcement.content";

async fn setting_string<C: sea_orm::ConnectionTrait>(
    db: &C,
    key: &str,
    op: &'static str,
) -> Result<Option<String>, AppError> {
    Ok(settings::get_setting(db, key)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .and_then(|setting| setting.value.as_str().map(str::to_owned)))
}

pub async fn get(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.announcements.get";
    let db = super::database(&state, OP)?;
    Ok(ok_response(json!({
        "title": setting_string(db, TITLE_KEY, OP).await?,
        "content": setting_string(db, CONTENT_KEY, OP).await?,
    })))
}

#[derive(Deserialize)]
pub struct PublishAnnouncementRequest {
    pub title: String,
    pub content: String,
}

fn prepare_announcement(
    body: PublishAnnouncementRequest,
    op: &'static str,
) -> Result<(String, String), AppError> {
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
    Ok((title, content))
}

pub async fn publish(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Json(body): Json<PublishAnnouncementRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.announcements.publish";
    let (title, content) = prepare_announcement(body, OP)?;
    let db = super::database(&state, OP)?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let before_title = setting_string(&transaction, TITLE_KEY, OP).await?;
    let before_content = setting_string(&transaction, CONTENT_KEY, OP).await?;

    settings::set_string(&transaction, TITLE_KEY, &title)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    settings::set_string(&transaction, CONTENT_KEY, &content)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    let recipients = notifications::create_announcement_notifications(
        &transaction,
        notifications::CreateAnnouncementNotification {
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
            target_id: None,
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "recipients": recipients }),
        },
        json!({
            "before": { "title": before_title, "content": before_content },
            "after": { "title": title, "content": content },
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
        "title": title,
        "content": content,
        "recipients": recipients,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announcement_input_is_trimmed_and_bounded() {
        let (title, content) = prepare_announcement(
            PublishAnnouncementRequest {
                title: "  Planned maintenance  ".to_owned(),
                content: "  The API will restart shortly.  ".to_owned(),
            },
            "test.announcement",
        )
        .unwrap();
        assert_eq!(title, "Planned maintenance");
        assert_eq!(content, "The API will restart shortly.");

        assert!(
            prepare_announcement(
                PublishAnnouncementRequest {
                    title: String::new(),
                    content: "content".to_owned(),
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
                },
                "test.announcement",
            )
            .is_err()
        );
    }
}
