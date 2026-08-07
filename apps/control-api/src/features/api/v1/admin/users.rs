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
        authentication::{self, UserMfaPolicy},
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
        "email_verified": user.email_verified_at.is_some(),
        "last_login_at": ts(user.last_login_at),
        "created_at": ts(user.created_at),
    })
}

#[derive(Deserialize)]
pub struct ListUsersQuery {
    pub q: Option<String>,
    pub limit: Option<u64>,
    pub status: Option<String>,
    pub role: Option<String>,
}

fn parse_user_status_filter(
    value: Option<&str>,
    op: &'static str,
) -> Result<Option<UserStatus>, AppError> {
    value
        .map(|value| {
            UserStatus::parse(value).ok_or_else(|| AppError::Validation {
                op,
                message: "status must be active or disabled".to_owned(),
            })
        })
        .transpose()
}

fn parse_user_role_filter(
    value: Option<&str>,
    op: &'static str,
) -> Result<Option<PlatformRole>, AppError> {
    value
        .map(|value| {
            PlatformRole::parse(value).ok_or_else(|| AppError::Validation {
                op,
                message: "role must be user or admin".to_owned(),
            })
        })
        .transpose()
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum UserBatchRequest {
    Enable { ids: Vec<Uuid> },
    Disable { ids: Vec<Uuid> },
}

/// POST /api/v1/admin/users/batch
pub async fn batch(
    State(state): State<ControlApiState>,
    session: Session,
    Json(body): Json<UserBatchRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.users.batch";
    let (ids, status) = match body {
        UserBatchRequest::Enable { ids } => (ids, "active"),
        UserBatchRequest::Disable { ids } => (ids, "disabled"),
    };
    let ids = super::batch::normalize_ids(ids, OP)?;
    let results = super::batch::run(ids, |user_id| {
        let state = state.clone();
        let session = session.clone();
        async move {
            update(
                State(state),
                session,
                Path(user_id),
                Json(UpdateUserRequest {
                    display_name: None,
                    status: Some(status.to_owned()),
                    platform_role: None,
                }),
            )
            .await
            .map(|_| ())
        }
    })
    .await;

    Ok(ok_response(json!({ "results": results })))
}

/// GET /api/v1/admin/users
pub async fn list(
    State(state): State<ControlApiState>,
    Query(query): Query<ListUsersQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.users.list";
    let db = super::database(&state, OP)?;
    let status = parse_user_status_filter(query.status.as_deref(), OP)?;
    let platform_role = parse_user_role_filter(query.role.as_deref(), OP)?;

    let users = users::list_users(
        db,
        UserListFilter {
            query: query.q,
            status,
            platform_role,
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
    let password_policy = authentication::password_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let generated = body.password.is_none();
    let password = match body.password {
        Some(password) => password,
        None => password_policy
            .generate_password()
            .map_err(|message| AppError::Validation {
                op: OP,
                message: message.to_owned(),
            })?,
    };
    password_policy
        .validate_password(&password)
        .map_err(|message| AppError::Validation {
            op: OP,
            message: message.to_owned(),
        })?;
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
            password_hash: Some(password_hash),
            platform_role,
            email_verified_at: Some(time::OffsetDateTime::now_utc()),
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

pub async fn mfa_factors(
    State(state): State<ControlApiState>,
    Path(user_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.users.mfa.list";
    let db = super::database(&state, OP)?;
    let user = users::get_user_by_id(db, user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "user not found".to_owned(),
        })?;
    let factors = authentication::mfa_factors(db, user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let platform_policy = authentication::mfa_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let user_policy = authentication::user_mfa_policy(db, user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let requirements = platform_policy.requirements_for(&user_policy, &user.platform_role);
    Ok(ok_response(json!({
        "factors": factors.iter().map(|factor| json!({
            "id": factor.id,
            "kind": factor.kind.as_str(),
            "label": factor.label,
            "verified": factor.verified_at.is_some(),
            "verified_at": ts(factor.verified_at),
            "last_used_at": ts(factor.last_used_at),
            "created_at": ts(factor.created_at),
        })).collect::<Vec<_>>(),
        "policy": user_policy,
        "allowed_factors": platform_policy.allowed_factors,
        "effective_requirements": {
            "minimum_factors": requirements.minimum_factors,
            "required_factors": requirements.required_factors.iter().map(|kind| kind.as_str()).collect::<Vec<_>>(),
        },
    })))
}

pub async fn update_mfa_policy(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(user_id): Path<Uuid>,
    Json(policy): Json<UserMfaPolicy>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.users.mfa.policy.update";
    let db = super::database(&state, OP)?;
    let user = users::get_user_by_id(db, user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "user not found".to_owned(),
        })?;
    let platform_policy = authentication::mfa_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    policy
        .validate(&platform_policy)
        .map_err(|message| AppError::Validation {
            op: OP,
            message: message.to_owned(),
        })?;
    authentication::set_user_mfa_policy(db, user_id, &policy)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let requirements = platform_policy.requirements_for(&policy, &user.platform_role);
    let _ = audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "user.mfa_policy_updated".to_owned(),
            target_type: "user".to_owned(),
            target_id: Some(user_id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({
                "inherit_platform": policy.inherit_platform,
                "minimum_factors": policy.minimum_factors,
                "required_factors": policy.required_factors,
            }),
        },
    )
    .await;
    Ok(ok_response(json!({
        "policy": policy,
        "effective_requirements": {
            "minimum_factors": requirements.minimum_factors,
            "required_factors": requirements.required_factors.iter().map(|kind| kind.as_str()).collect::<Vec<_>>(),
        },
    })))
}

pub async fn reset_mfa_factor(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path((user_id, factor_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.users.mfa.reset";
    let db = super::database(&state, OP)?;
    let factor = authentication::mfa_factor(db, user_id, factor_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "MFA factor not found".to_owned(),
        })?;
    authentication::delete_mfa_factor(db, user_id, factor_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let _ = audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "user.mfa_factor_reset".to_owned(),
            target_type: "user".to_owned(),
            target_id: Some(user_id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "factor_id": factor.id, "factor_kind": factor.kind.as_str() }),
        },
    )
    .await;
    Ok(ok_response(json!({ "deleted": true })))
}

#[cfg(test)]
mod batch_and_filter_tests {
    use super::*;

    #[test]
    fn user_filters_are_typed_and_reject_unknown_values() {
        assert_eq!(
            parse_user_status_filter(Some("disabled"), "test.users").unwrap(),
            Some(UserStatus::Disabled)
        );
        assert_eq!(
            parse_user_role_filter(Some("admin"), "test.users").unwrap(),
            Some(PlatformRole::Admin)
        );
        assert!(parse_user_status_filter(Some("deleted"), "test.users").is_err());
        assert!(parse_user_role_filter(Some("owner"), "test.users").is_err());
    }

    #[test]
    fn user_batch_actions_are_resource_specific() {
        let id = Uuid::now_v7();
        let request: UserBatchRequest = serde_json::from_value(json!({
            "action": "disable",
            "ids": [id],
        }))
        .unwrap();
        assert!(matches!(request, UserBatchRequest::Disable { .. }));
        assert!(
            serde_json::from_value::<UserBatchRequest>(json!({
                "action": "delete",
                "ids": [id],
            }))
            .is_err()
        );
    }
}
