use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::{ConnectionTrait, TransactionTrait};
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::{
        teams::CreateTeamParams,
        users::{self, CreateUserParams},
    },
    infra::{
        database::entity::{PlatformRole, TeamKind},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

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
    let db = super::setup_database(&state, "setup.admin.database")?;
    super::ensure_setup_mutation_allowed(db, "setup.admin.ready_mode").await?;

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
            password_hash,
            platform_role: initial_platform_role(),
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

    Ok(ok_response(json!({
        "user": {
            "id": user.id,
            "email": user.email,
            "display_name": user.display_name,
            "platform_role": user.platform_role.as_str(),
        },
        "team": {
            "id": team.id,
            "slug": team.slug,
            "name": team.name,
        },
    })))
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

#[cfg(test)]
mod tests {
    use super::*;

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
