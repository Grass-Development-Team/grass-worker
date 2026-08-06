use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::audits::{self, CreateAuditEventParams},
    infra::{
        database::entity::{AuditEventResult, registration_email_allowlist, user},
        error::{AppError, ok_response},
        http::{extractors::Session, timestamps::ts},
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct AddEmailRequest {
    pub email: String,
}

fn validate_email(value: &str) -> Result<String, AppError> {
    grass_validator::normalize_email(value).map_err(|error| AppError::Validation {
        op: "admin.registration.emails.validate",
        message: error.to_string(),
    })
}

pub async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.registration.emails.list";
    let db = super::database(&state, OP)?;
    let entries = registration_email_allowlist::Entity::find()
        .order_by_desc(registration_email_allowlist::Column::CreatedAt)
        .order_by_desc(registration_email_allowlist::Column::Id)
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let creator_ids = entries
        .iter()
        .filter_map(|entry| entry.created_by_user_id)
        .collect::<Vec<_>>();
    let creators = if creator_ids.is_empty() {
        Vec::new()
    } else {
        user::Entity::find()
            .filter(user::Column::Id.is_in(creator_ids))
            .all(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
    };
    let creators = creators
        .into_iter()
        .map(|user| (user.id, user))
        .collect::<HashMap<_, _>>();

    Ok(ok_response(json!({
        "emails": entries.iter().map(|entry| {
            let creator = entry.created_by_user_id.and_then(|id| creators.get(&id));
            json!({
                "id": entry.id,
                "email": entry.email,
                "created_at": ts(entry.created_at),
                "created_by": creator.map(|user| json!({
                    "id": user.id,
                    "email": user.email,
                    "display_name": user.display_name,
                })),
            })
        }).collect::<Vec<_>>(),
    })))
}

pub async fn add(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Json(body): Json<AddEmailRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.registration.emails.add";
    let db = super::database(&state, OP)?;
    let email = validate_email(&body.email)?;
    let entry = registration_email_allowlist::ActiveModel {
        id: Set(Uuid::now_v7()),
        email: Set(email),
        created_by_user_id: Set(Some(data.user_id)),
        created_at: Set(OffsetDateTime::now_utc()),
    }
    .insert(db)
    .await
    .map_err(|source| {
        let source = anyhow::Error::from(source);
        if crate::infra::database::is_unique_violation(&source) {
            AppError::Conflict {
                op: OP,
                message: "email is already allowed to register".to_owned(),
            }
        } else {
            AppError::Infrastructure { op: OP, source }
        }
    })?;
    let _ = audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "registration.email_allowed".to_owned(),
            target_type: "registration_email".to_owned(),
            target_id: Some(entry.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "email": entry.email }),
        },
    )
    .await;

    Ok(ok_response(json!({
        "email": {
            "id": entry.id,
            "email": entry.email,
            "created_at": ts(entry.created_at),
            "created_by": {
                "id": data.user_id,
            },
        }
    })))
}

pub async fn remove(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(entry_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.registration.emails.remove";
    let db = super::database(&state, OP)?;
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
    let _ = audits::create_platform_audit_event(
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
    .await;

    Ok(ok_response(json!({ "deleted": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_input_is_normalized_as_an_exact_email() {
        assert_eq!(
            validate_email("  User@Example.COM ").unwrap(),
            "user@example.com"
        );
        assert!(validate_email("not-an-email").is_err());
    }
}
