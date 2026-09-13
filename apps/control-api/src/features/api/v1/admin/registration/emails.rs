pub(crate) mod by_entry_id;

use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, registration_email_allowlist, user},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/registration/emails", axum::routing::get(list).post(add))
        .merge(by_entry_id::router())
}

#[derive(Deserialize)]
struct AddEmailRequest {
    email: String,
}

fn validate_email(value: &str) -> Result<String, AppError> {
    grass_validator::normalize_email(value).map_err(|error| AppError::Validation {
        op: "admin.registration.emails.validate",
        message: error.to_string(),
    })
}

async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.registration.emails.list";
    let db = crate::infra::http::database(&state, OP)?;
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

    Ok(ok_response(ListResponse {
        emails: entries
            .iter()
            .map(|entry| {
                let creator = entry.created_by_user_id.and_then(|id| creators.get(&id));
                ListEmailsResponse {
                    id: entry.id,
                    email: entry.email.clone(),
                    created_at: entry.created_at,
                    created_by: creator.map(|user| ListEmailsCreatedByResponse {
                        id: user.id,
                        email: user.email.clone(),
                        display_name: user.display_name.clone(),
                    }),
                }
            })
            .collect::<Vec<_>>(),
    }))
}

async fn add(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Json(body): Json<AddEmailRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.registration.emails.add";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;
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
    audits::create_platform_audit_event(
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
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(AddResponse {
        email: AddEmailResponse {
            id: entry.id,
            email: entry.email.clone(),
            created_at: entry.created_at,
            created_by: AddEmailCreatedByResponse { id: data.user_id },
        },
    }))
}

#[derive(serde::Serialize)]
struct ListEmailsCreatedByResponse {
    id: uuid::Uuid,
    email: String,
    display_name: Option<String>,
}

#[derive(serde::Serialize)]
struct ListEmailsResponse {
    id: uuid::Uuid,
    email: String,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
    created_by: Option<ListEmailsCreatedByResponse>,
}

#[derive(serde::Serialize)]
struct ListResponse {
    emails: Vec<ListEmailsResponse>,
}

#[derive(serde::Serialize)]
struct AddEmailCreatedByResponse {
    id: uuid::Uuid,
}

#[derive(serde::Serialize)]
struct AddEmailResponse {
    id: uuid::Uuid,
    email: String,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
    created_by: AddEmailCreatedByResponse,
}

#[derive(serde::Serialize)]
struct AddResponse {
    email: AddEmailResponse,
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
