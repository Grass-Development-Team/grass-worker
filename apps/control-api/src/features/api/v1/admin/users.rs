use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use sea_orm::TransactionTrait;

use crate::infra::http::timestamps::ts;
use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        teams::{self, CreateTeamParams},
        users::{self, CreateUserParams, UpdateUserParams, UserListFilter},
    },
    infra::{
        database::entity::{AuditEventResult, PlatformRole, TeamKind, UserStatus, user},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

fn user_view(user: &user::Model) -> serde_json::Value {
    json!({
        "id": user.id,
        "email": user.email,
        "display_name": user.display_name,
        "status": user.status.as_str(),
        "platform_role": user.platform_role.as_str(),
        "last_login_at": ts(user.last_login_at),
        "created_at": ts(user.created_at),
    })
}

#[derive(Deserialize)]
pub struct ListUsersQuery {
    pub q: Option<String>,
    pub limit: Option<u64>,
}

/// GET /api/v1/admin/users
pub async fn list(
    State(state): State<ControlApiState>,
    Query(query): Query<ListUsersQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.users.list";
    let db = super::database(&state, OP)?;

    let users = users::list_users(
        db,
        UserListFilter {
            query: query.q,
            limit: query.limit.unwrap_or(100),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(json!({
        "users": users.iter().map(user_view).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct UpdateUserRequest {
    /// Explicit `null` clears the display name.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub display_name: Option<Option<String>>,
    pub status: Option<String>,
    pub platform_role: Option<String>,
}

fn deserialize_double_option<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    Option::<String>::deserialize(deserializer).map(Some)
}

/// PATCH /api/v1/admin/users/{user_id}
pub async fn update(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(user_id): Path<Uuid>,
    Json(body): Json<UpdateUserRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.users.update";
    let db = super::database(&state, OP)?;

    let target = users::get_user_by_id(db, user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "user not found".to_owned(),
        })?;

    let status = body
        .status
        .as_deref()
        .map(|value| {
            UserStatus::parse(value).ok_or_else(|| AppError::Validation {
                op: OP,
                message: format!("unknown user status: {value}"),
            })
        })
        .transpose()?;
    let platform_role = body
        .platform_role
        .as_deref()
        .map(|value| {
            PlatformRole::parse(value).ok_or_else(|| AppError::Validation {
                op: OP,
                message: format!("unknown platform role: {value}"),
            })
        })
        .transpose()?;
    let display_name = body.display_name.map(|name| {
        name.map(|value| value.trim().to_owned())
            .filter(|v| !v.is_empty())
    });
    if display_name
        .as_ref()
        .and_then(|name| name.as_ref())
        .is_some_and(|name| name.chars().count() > 120)
    {
        return Err(AppError::Validation {
            op: OP,
            message: "display name must not exceed 120 characters".to_owned(),
        });
    }

    let demotes_admin = target.platform_role == PlatformRole::Admin
        && matches!(platform_role, Some(PlatformRole::User));
    let disables_user = matches!(status, Some(UserStatus::Disabled));

    if target.id == data.user_id && (demotes_admin || disables_user) {
        return Err(AppError::Validation {
            op: OP,
            message: "you cannot disable or demote your own account".to_owned(),
        });
    }
    if target.platform_role == PlatformRole::Admin
        && target.status == UserStatus::Active
        && (demotes_admin || disables_user)
    {
        let admins = users::count_active_admins(db)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        if admins <= 1 {
            return Err(AppError::Validation {
                op: OP,
                message: "the platform must keep at least one active administrator".to_owned(),
            });
        }
    }

    let mut changed: Vec<&'static str> = Vec::new();
    if display_name.is_some() {
        changed.push("display_name");
    }
    if status.is_some() {
        changed.push("status");
    }
    if platform_role.is_some() {
        changed.push("platform_role");
    }

    let updated = users::update_user(
        db,
        target,
        UpdateUserParams {
            display_name,
            status,
            platform_role,
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    let _ = audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "user.updated".to_owned(),
            target_type: "user".to_owned(),
            target_id: Some(updated.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "changed": changed }),
        },
    )
    .await;

    Ok(ok_response(json!({ "user": user_view(&updated) })))
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
    let db = super::database(&state, OP)?;

    let target = users::get_user_by_id(db, user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "user not found".to_owned(),
        })?;

    let requested = body.and_then(|Json(body)| body.password);
    if let Some(password) = requested.as_deref()
        && !(8..=1024).contains(&password.len())
    {
        return Err(AppError::Validation {
            op: OP,
            message: "password must contain between 8 and 1024 bytes".to_owned(),
        });
    }
    let generated = requested.is_none();
    let password = requested.unwrap_or_else(grass_token::generate_token);
    let password_hash =
        grass_crypto::hash_password(&password).map_err(|error| AppError::Internal {
            op: OP,
            message: format!("password hashing failed: {error}"),
        })?;
    users::set_password(db, target.id, password_hash)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    let _ = audits::create_platform_audit_event(
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
    .await;

    Ok(ok_response(json!({
        "user_id": target.id,
        // Present only when generated; shown exactly once.
        "password": generated.then_some(password),
    })))
}

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub platform_role: Option<String>,
    /// Omitted: a strong password is generated and returned once.
    #[serde(default)]
    pub password: Option<String>,
}

/// POST /api/v1/admin/users — provisions an account exactly like signup
/// (personal team included) without touching the signup policy.
pub async fn create(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Json(body): Json<CreateUserRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.users.create";
    let db = super::database(&state, OP)?;

    let email =
        grass_validator::normalize_email(&body.email).map_err(|error| AppError::Validation {
            op: OP,
            message: error.to_string(),
        })?;
    let display_name = body
        .display_name
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());
    if display_name
        .as_ref()
        .is_some_and(|name| name.chars().count() > 120)
    {
        return Err(AppError::Validation {
            op: OP,
            message: "display name must not exceed 120 characters".to_owned(),
        });
    }
    let platform_role = match body.platform_role.as_deref() {
        None => PlatformRole::User,
        Some(value) => PlatformRole::parse(value).ok_or_else(|| AppError::Validation {
            op: OP,
            message: format!("unknown platform role: {value}"),
        })?,
    };
    if let Some(password) = body.password.as_deref()
        && !(8..=1024).contains(&password.len())
    {
        return Err(AppError::Validation {
            op: OP,
            message: "password must contain between 8 and 1024 bytes".to_owned(),
        });
    }
    let generated = body.password.is_none();
    let password = body.password.unwrap_or_else(grass_token::generate_token);
    let password_hash =
        grass_crypto::hash_password(&password).map_err(|error| AppError::Internal {
            op: OP,
            message: format!("password hashing failed: {error}"),
        })?;

    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let created = users::create_user(
        &transaction,
        CreateUserParams {
            email: email.clone(),
            display_name: display_name.clone(),
            password_hash,
            platform_role,
        },
    )
    .await
    .map_err(|source| {
        if crate::infra::database::is_unique_violation(&source) {
            AppError::Conflict {
                op: OP,
                message: "an account with this email already exists".to_owned(),
            }
        } else {
            AppError::Infrastructure { op: OP, source }
        }
    })?;

    let slug = format!(
        "{}-{}",
        crate::features::api::v1::auth::register::personal_team_slug(&email),
        &created.id.simple().to_string()[..8]
    );
    teams::create_team_with_connection(
        &transaction,
        CreateTeamParams {
            slug,
            name: format!("{}'s Team", display_name.as_deref().unwrap_or("User")),
            kind: TeamKind::Personal,
            owner_user_id: created.id,
            group_id: None,
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

    let _ = audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "user.created".to_owned(),
            target_type: "user".to_owned(),
            target_id: Some(created.id),
            result: AuditEventResult::Success,
            reason: Some("created by platform administrator".to_owned()),
            metadata: json!({ "email": created.email, "role": created.platform_role.as_str() }),
        },
    )
    .await;

    Ok(ok_response(json!({
        "user": user_view(&created),
        // Present only when generated; shown exactly once.
        "password": generated.then_some(password),
    })))
}
