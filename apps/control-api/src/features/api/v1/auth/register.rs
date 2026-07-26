use axum::{Json, extract::State, response::IntoResponse};
use axum_extra::extract::cookie::CookieJar;
use sea_orm::{QuerySelect, TransactionTrait, entity::prelude::*};
use serde::Deserialize;

use crate::{
    domain::{
        settings,
        teams::{self, AcceptInvitationParams, CreateTeamParams, InvitationError},
        users::{self, CreateUserParams},
    },
    infra::{
        database::entity::{PlatformRole, TeamKind, team_invitation},
        error::AppError,
    },
    state::ControlApiState,
};

const SIGNUP_POLICY_KEY: &str = "signup.policy";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SignupPolicy {
    Open,
    InviteOnly,
    Closed,
}

struct RegistrationInput {
    email: String,
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
    pub invitation_token: Option<String>,
}

pub async fn handler(
    State(state): State<ControlApiState>,
    jar: CookieJar,
    Json(body): Json<RegisterRequest>,
) -> Result<impl IntoResponse, AppError> {
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: "auth.register.no_database",
        message: "database not available".to_owned(),
    })?;
    let cache = state.try_cache().ok_or_else(|| AppError::Internal {
        op: "auth.register.no_cache",
        message: "cache service not available".to_owned(),
    })?;
    let input = validate_registration_input(&body.email, &body.password)?;
    let display_name = body
        .display_name
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .or_else(|| input.email.split('@').next().map(str::to_owned));
    if display_name
        .as_ref()
        .is_some_and(|display_name| display_name.chars().count() > 120)
    {
        return Err(AppError::Validation {
            op: "auth.register.display_name_too_long",
            message: "display name must not exceed 120 characters".to_owned(),
        });
    }
    let policy_setting = settings::get_setting(db, SIGNUP_POLICY_KEY)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.register.read_policy",
            source,
        })?;
    let policy_value = policy_setting
        .as_ref()
        .and_then(|setting| setting.value.as_str());
    let policy = signup_policy(policy_value)?;
    if policy == SignupPolicy::Closed {
        return Err(AppError::Forbidden {
            op: "auth.register.closed",
            message: "user registration is closed".to_owned(),
        });
    }

    let invitation_token = body
        .invitation_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty());
    if policy == SignupPolicy::InviteOnly && invitation_token.is_none() {
        return Err(AppError::Forbidden {
            op: "auth.register.invitation_required",
            message: "a valid invitation is required".to_owned(),
        });
    }

    let password_hash =
        grass_crypto::hash_password(&body.password).map_err(|error| AppError::Internal {
            op: "auth.register.hash_password",
            message: format!("password hashing failed: {error}"),
        })?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.register.begin_transaction",
            source: source.into(),
        })?;

    if users::get_user_by_email(&transaction, &input.email)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.register.check_email",
            source,
        })?
        .is_some()
    {
        return Err(AppError::Conflict {
            op: "auth.register.email_exists",
            message: "an account with this email already exists".to_owned(),
        });
    }

    if let Some(token) = invitation_token {
        validate_invitation(&transaction, token, &input.email).await?;
    }

    let user = users::create_user(
        &transaction,
        CreateUserParams {
            email: input.email.clone(),
            display_name: display_name.clone(),
            password_hash,
            platform_role: registration_platform_role(),
        },
    )
    .await
    .map_err(|source| {
        if crate::infra::database::is_unique_violation(&source) {
            AppError::Conflict {
                op: "auth.register.email_exists",
                message: "an account with this email already exists".to_owned(),
            }
        } else {
            AppError::Infrastructure {
                op: "auth.register.create_user",
                source,
            }
        }
    })?;

    let slug = format!(
        "{}-{}",
        personal_team_slug(&input.email),
        &user.id.simple().to_string()[..8]
    );
    teams::create_team_with_connection(
        &transaction,
        CreateTeamParams {
            slug,
            name: format!("{}'s Team", display_name.as_deref().unwrap_or("User")),
            kind: TeamKind::Personal,
            owner_user_id: user.id,
            group_id: None,
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure {
        op: "auth.register.create_personal_team",
        source,
    })?;

    if let Some(token) = invitation_token {
        teams::accept_invitation_with_connection(
            &transaction,
            AcceptInvitationParams {
                token_hash: teams::invitation_token_hash(token),
                user_id: user.id,
            },
        )
        .await
        .map_err(map_invitation_error)?;
    }

    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.register.commit_transaction",
            source: source.into(),
        })?;

    super::login::authenticated_response(&state, cache, jar, user).await
}

fn registration_platform_role() -> PlatformRole {
    PlatformRole::User
}

fn signup_policy(value: Option<&str>) -> Result<SignupPolicy, AppError> {
    match value.unwrap_or("open") {
        "open" => Ok(SignupPolicy::Open),
        "invite_only" => Ok(SignupPolicy::InviteOnly),
        "closed" => Ok(SignupPolicy::Closed),
        _ => Err(AppError::Internal {
            op: "auth.register.invalid_policy",
            message: "signup policy setting is invalid".to_owned(),
        }),
    }
}

#[cfg(test)]
mod platform_role_tests {
    use super::*;

    #[test]
    fn registration_creates_a_regular_platform_user() {
        assert_eq!(registration_platform_role(), PlatformRole::User);
    }
}

fn validate_registration_input(email: &str, password: &str) -> Result<RegistrationInput, AppError> {
    let email = grass_validator::normalize_email(email).map_err(|error| AppError::Validation {
        op: "auth.register.invalid_email",
        message: error.to_string(),
    })?;
    if !(8..=1024).contains(&password.len()) {
        return Err(AppError::Validation {
            op: "auth.register.invalid_password_length",
            message: "password must contain between 8 and 1024 bytes".to_owned(),
        });
    }
    Ok(RegistrationInput { email })
}

async fn validate_invitation<C: ConnectionTrait>(
    db: &C,
    token: &str,
    email: &str,
) -> Result<(), AppError> {
    let invitation = team_invitation::Entity::find()
        .filter(team_invitation::Column::TokenHash.eq(teams::invitation_token_hash(token)))
        .lock_exclusive()
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.register.read_invitation",
            source: source.into(),
        })?
        .ok_or_else(|| map_invitation_error(InvitationError::NotFound))?;
    teams::validate_invitation_acceptance(
        &invitation.status,
        invitation.expires_at,
        &invitation.email,
        email,
        time::OffsetDateTime::now_utc(),
    )
    .map_err(map_invitation_error)
}

fn map_invitation_error(error: InvitationError) -> AppError {
    match error {
        InvitationError::NotFound => AppError::Forbidden {
            op: "auth.register.invitation_not_found",
            message: "invitation is invalid".to_owned(),
        },
        InvitationError::NotPending => AppError::Conflict {
            op: "auth.register.invitation_not_pending",
            message: error.to_string(),
        },
        InvitationError::Expired => AppError::Gone {
            op: "auth.register.invitation_expired",
            message: error.to_string(),
        },
        InvitationError::EmailMismatch => AppError::Forbidden {
            op: "auth.register.invitation_email_mismatch",
            message: error.to_string(),
        },
        InvitationError::OwnerRole | InvitationError::AlreadyMember => AppError::Conflict {
            op: "auth.register.invitation_conflict",
            message: error.to_string(),
        },
        InvitationError::Database(source) => AppError::Infrastructure {
            op: "auth.register.accept_invitation",
            source: source.into(),
        },
    }
}

fn personal_team_slug(email: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_signup_policy_defaults_to_open() {
        assert_eq!(signup_policy(None).unwrap(), SignupPolicy::Open);
    }

    #[test]
    fn parses_supported_signup_policies() {
        assert_eq!(signup_policy(Some("open")).unwrap(), SignupPolicy::Open);
        assert_eq!(
            signup_policy(Some("invite_only")).unwrap(),
            SignupPolicy::InviteOnly
        );
        assert_eq!(signup_policy(Some("closed")).unwrap(), SignupPolicy::Closed);
        assert!(signup_policy(Some("unexpected")).is_err());
    }

    #[test]
    fn normalizes_and_validates_registration_input() {
        let input = validate_registration_input("  User@Example.COM ", "password").unwrap();
        assert_eq!(input.email, "user@example.com");
        assert!(validate_registration_input("invalid", "password").is_err());
        assert!(validate_registration_input("user@example.com", "short").is_err());
    }

    #[test]
    fn personal_team_slug_is_stable_and_safe() {
        assert_eq!(
            personal_team_slug("User.Name+tag@example.com"),
            "usernametag"
        );
        assert_eq!(personal_team_slug("---@example.com"), "user");
    }
}
