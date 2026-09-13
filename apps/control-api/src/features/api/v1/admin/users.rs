pub(crate) mod batch;
pub(crate) mod by_user_id;

use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;

use crate::infra::audit as audits;
use crate::{
    domain::{
        authentication,
        teams::{self, CreateTeamParams},
        users::{self, CreateUserParams, UserListFilter},
    },
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, PlatformRole, TeamKind, UserStatus, user},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/users", axum::routing::get(list).post(create))
        .merge(batch::router())
        .merge(by_user_id::router())
}

fn user_view(user: &user::Model) -> UserResponse {
    UserResponse {
        id: user.id,
        email: user.email.clone(),
        display_name: user.display_name.clone(),
        status: user.status.as_str(),
        platform_role: user.platform_role.as_str(),
        email_verified: user.email_verified_at.is_some(),
        last_login_at: user.last_login_at,
        created_at: user.created_at,
    }
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

/// GET /api/v1/admin/users
pub async fn list(
    State(state): State<ControlApiState>,
    Query(query): Query<ListUsersQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.users.list";
    let db = crate::infra::http::database(&state, OP)?;
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

    Ok(ok_response(ListResponse {
        users: users.iter().map(user_view).collect::<Vec<_>>(),
    }))
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
    let db = crate::infra::http::database(&state, OP)?;

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

    let transaction = crate::infra::audit::AuditTransaction::begin(db)
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
        personal_team_slug(&email),
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

    audits::create_platform_audit_event(
        &transaction,
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
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(CreateResponse {
        user: user_view(&created),
        password: generated.then_some(password),
    }))
}

pub(crate) fn personal_team_slug(email: &str) -> String {
    let slug = email
        .split('@')
        .next()
        .unwrap_or("user")
        .to_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(40)
        .collect::<String>();
    if slug.is_empty() {
        "user".to_owned()
    } else {
        slug
    }
}

#[derive(serde::Serialize)]
struct UserResponse {
    id: uuid::Uuid,
    email: String,
    display_name: Option<String>,
    status: &'static str,
    platform_role: &'static str,
    email_verified: bool,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    last_login_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct ListResponse {
    users: Vec<UserResponse>,
}

#[derive(serde::Serialize)]
struct CreateResponse {
    user: UserResponse,
    password: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::infra::database::entity::PlatformRole;
    use crate::infra::database::entity::UserStatus;
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
}
