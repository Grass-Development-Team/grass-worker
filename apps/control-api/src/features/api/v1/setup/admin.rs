use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::{
        teams::CreateTeamParams,
        users::{self, CreateUserParams},
    },
    infra::{
        database::entity::TeamKind,
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
    let db = super::setup_database(&state, "setup.admin.database")?;
    super::ensure_setup_mutation_allowed(db, "setup.admin.ready_mode").await?;

    if users::any_user_exists(db)
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

    if body.email.trim().is_empty() || !body.email.contains('@') {
        return Err(AppError::Validation {
            op: "setup.admin.invalid_email",
            message: "invalid email address".to_owned(),
        });
    }

    if body.password.len() < 8 {
        return Err(AppError::Validation {
            op: "setup.admin.password_too_short",
            message: "password must be at least 8 characters".to_owned(),
        });
    }

    let password_hash =
        grass_crypto::hash_password(&body.password).map_err(|error| AppError::Internal {
            op: "setup.admin.hash_password",
            message: format!("password hashing failed: {error}"),
        })?;

    let email = body.email.trim().to_lowercase();
    let user = users::create_user(
        db,
        CreateUserParams {
            email: email.clone(),
            display_name: body
                .display_name
                .filter(|name| !name.trim().is_empty())
                .or_else(|| email.split('@').next().map(str::to_owned)),
            password_hash,
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure {
        op: "setup.admin.create_user",
        source,
    })?;

    let team = crate::domain::teams::create_team(
        db,
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

    Ok(ok_response(json!({
        "user": {
            "id": user.id,
            "email": user.email,
            "display_name": user.display_name,
        },
        "team": {
            "id": team.id,
            "slug": team.slug,
            "name": team.name,
        },
    })))
}

fn make_slug(email: &str) -> String {
    let slug = email
        .split('@')
        .next()
        .unwrap_or("user")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(50)
        .collect::<String>();

    if slug.is_empty() {
        "user".to_owned()
    } else {
        slug
    }
}
