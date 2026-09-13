use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{authentication, users},
    infra::{
        audit::CreateAuditEventParams,
        database::entity::AuditEventResult,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/users/{user_id}/reset-password",
        axum::routing::post(reset_password),
    )
}

#[derive(Default, Deserialize)]
pub struct ResetPasswordRequest {
    /// Omitted: a strong password is generated and returned once.
    #[serde(default)]
    pub password: Option<String>,
}

/// POST /api/v1/admin/users/{user_id}/reset-password
///
/// Sets the given password, or issues a strong random one shown exactly
/// once. Plaintext is never returned for administrator-chosen passwords.
pub async fn reset_password(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(user_id): Path<Uuid>,
    body: Option<Json<ResetPasswordRequest>>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.users.reset_password";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;

    let target = users::get_user_by_id(db, user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "user not found".to_owned(),
        })?;

    let policy = authentication::password_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let requested = body.and_then(|Json(body)| body.password);
    let generated = requested.is_none();
    let password = match requested {
        Some(password) => password,
        None => policy
            .generate_password()
            .map_err(|message| AppError::Validation {
                op: OP,
                message: message.to_owned(),
            })?,
    };
    policy
        .validate_password(&password)
        .map_err(|message| AppError::Validation {
            op: OP,
            message: message.to_owned(),
        })?;
    if authentication::password_was_used_recently(db, target.id, &password, policy.history_count)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
    {
        return Err(AppError::Validation {
            op: OP,
            message: "password was used recently".to_owned(),
        });
    }
    let password_hash =
        grass_crypto::hash_password(&password).map_err(|error| AppError::Internal {
            op: OP,
            message: format!("password hashing failed: {error}"),
        })?;
    users::set_password(&**db, target.id, password_hash)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "user.password_reset".to_owned(),
            target_type: "user".to_owned(),
            target_id: Some(target.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({}),
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

    Ok(ok_response(ResetPasswordResponse {
        user_id: target.id,
        password: generated.then_some(password),
    }))
}

#[derive(serde::Serialize)]
struct ResetPasswordResponse {
    user_id: uuid::Uuid,
    password: Option<String>,
}
