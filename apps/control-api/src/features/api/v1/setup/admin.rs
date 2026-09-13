use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait};
use serde::Deserialize;

use crate::{
    domain::{
        teams::CreateTeamParams,
        users::{self, CreateUserParams},
    },
    infra::{
        database::entity::{PlatformRole, TeamKind},
        error::{AppError, ok_response},
    },
    init,
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/admin", axum::routing::post(handler))
}

pub(crate) fn setup_database<'a>(
    state: &'a ControlApiState,
    op: &'static str,
) -> Result<&'a DatabaseConnection, AppError> {
    state.try_database().ok_or_else(|| AppError::Validation {
        op,
        message: "database must be configured first".to_owned(),
    })
}

pub(crate) async fn ensure_setup_mutation_allowed(
    db: &DatabaseConnection,
    op: &'static str,
) -> Result<(), AppError> {
    if init::is_setup_finished(db)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
    {
        return Err(AppError::SetupNotAllowed {
            op,
            message: "setup has already finished".to_owned(),
        });
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct AdminSetupRequest {
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
}

pub async fn handler(
    State(state): State<ControlApiState>,
    Json(body): Json<AdminSetupRequest>,
) -> Result<impl IntoResponse, AppError> {
    let _setup_guard = state.lock_setup().await;
    let db = setup_database(&state, "setup.admin.database")?;
    ensure_setup_mutation_allowed(db, "setup.admin.ready_mode").await?;

    let email =
        grass_validator::normalize_email(&body.email).map_err(|error| AppError::Validation {
            op: "setup.admin.invalid_email",
            message: error.to_string(),
        })?;

    if !(8..=1024).contains(&body.password.len()) {
        return Err(AppError::Validation {
            op: "setup.admin.invalid_password_length",
            message: "password must contain between 8 and 1024 bytes".to_owned(),
        });
    }
    let display_name = body
        .display_name
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());
    if display_name
        .as_ref()
        .is_some_and(|display_name| display_name.chars().count() > 120)
    {
        return Err(AppError::Validation {
            op: "setup.admin.display_name_too_long",
            message: "display name must not exceed 120 characters".to_owned(),
        });
    }

    let password_hash =
        grass_crypto::hash_password(&body.password).map_err(|error| AppError::Internal {
            op: "setup.admin.hash_password",
            message: format!("password hashing failed: {error}"),
        })?;

    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.admin.begin_transaction",
            source: source.into(),
        })?;
    transaction
        .execute_unprepared(
            "SELECT pg_advisory_xact_lock(hashtext(current_database()), 1196578381)",
        )
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.admin.lock",
            source: source.into(),
        })?;

    if users::any_user_exists(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.admin.check_existing",
            source,
        })?
    {
        return Err(AppError::Conflict {
            op: "setup.admin.already_created",
            message: "initial admin already exists".to_owned(),
        });
    }

    let user = users::create_user(
        &transaction,
        CreateUserParams {
            email: email.clone(),
            display_name: display_name.or_else(|| email.split('@').next().map(str::to_owned)),
            password_hash: Some(password_hash),
            platform_role: initial_platform_role(),
            email_verified_at: Some(time::OffsetDateTime::now_utc()),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure {
        op: "setup.admin.create_user",
        source,
    })?;

    let team = crate::domain::teams::create_team_with_connection(
        &transaction,
        CreateTeamParams {
            slug: make_slug(&email),
            name: format!("{}'s Team", user.display_name.as_deref().unwrap_or("Admin")),
            kind: TeamKind::Personal,
            owner_user_id: user.id,
            group_id: None,
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure {
        op: "setup.admin.create_team",
        source,
    })?;

    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.admin.commit",
            source: source.into(),
        })?;

    Ok(ok_response(ResponseBody {
        user: ResponseBodyUserResponse {
            id: user.id,
            email: user.email.clone(),
            display_name: user.display_name.clone(),
            platform_role: (user.platform_role.as_str()).to_owned(),
        },
        team: ResponseBodyTeamResponse {
            id: team.id,
            slug: team.slug.clone(),
            name: team.name.clone(),
        },
    }))
}

fn initial_platform_role() -> PlatformRole {
    PlatformRole::Admin
}

fn make_slug(email: &str) -> String {
    let candidate = email
        .split('@')
        .next()
        .unwrap_or("user")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .take(50)
        .collect::<String>();

    grass_validator::normalize_slug(&candidate).unwrap_or_else(|_| "user".to_owned())
}

#[derive(serde::Serialize)]
struct ResponseBodyUserResponse {
    id: uuid::Uuid,
    email: String,
    display_name: Option<String>,
    platform_role: String,
}

#[derive(serde::Serialize)]
struct ResponseBodyTeamResponse {
    id: uuid::Uuid,
    slug: String,
    name: String,
}

#[derive(serde::Serialize)]
struct ResponseBody {
    user: ResponseBodyUserResponse,
    team: ResponseBodyTeamResponse,
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::infra::database::entity::PlatformRole;
    #[test]
    fn setup_creates_a_platform_administrator() {
        assert_eq!(initial_platform_role(), PlatformRole::Admin);
    }

    #[test]
    fn initial_team_slug_is_normalized() {
        assert_eq!(make_slug("User.Name+tag@example.com"), "user-name-tag");
        assert_eq!(make_slug("---@example.com"), "user");
    }
}
