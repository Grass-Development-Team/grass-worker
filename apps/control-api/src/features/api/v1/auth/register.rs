use axum::{
    Json,
    extract::State,
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::CookieJar;
use sea_orm::TransactionTrait;
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::{
        authentication, platform_mail, registration, settings,
        teams::{self, CreateTeamParams},
        users::{self, CreateUserParams},
    },
    infra::{
        database::entity::{AuthTokenKind, PlatformRole, TeamKind},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

struct RegistrationInput {
    email: String,
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
    pub return_to: Option<String>,
    pub registration_code: Option<String>,
}

pub async fn handler(
    State(state): State<ControlApiState>,
    jar: CookieJar,
    Json(body): Json<RegisterRequest>,
) -> Result<Response, AppError> {
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: "auth.register.no_database",
        message: "database not available".to_owned(),
    })?;
    let cache = state.try_cache().ok_or_else(|| AppError::Internal {
        op: "auth.register.no_cache",
        message: "cache service not available".to_owned(),
    })?;
    let password_policy = authentication::password_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.register.password_policy",
            source,
        })?;
    password_policy
        .validate_password(&body.password)
        .map_err(|message| AppError::Validation {
            op: "auth.register.password_policy",
            message: message.to_owned(),
        })?;
    let input = validate_registration_input(&body.email)?;
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
    let policy_setting = settings::get_setting(db, "signup.policy")
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.register.read_policy",
            source,
        })?;
    let policy_value = policy_setting
        .as_ref()
        .and_then(|setting| setting.value.as_str());
    let policy = registration::SignupPolicy::parse(policy_value)
        .map_err(|error| map_registration_access_error(error, "auth.register.invalid_policy"))?;

    let return_to = super::oidc::safe_return_to(body.return_to.as_deref());
    let registration_code = body
        .registration_code
        .as_deref()
        .map(str::trim)
        .filter(|code| !code.is_empty());
    let verification_required = authentication::registration_verification_required(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.register.verification_policy",
            source,
        })?;
    let mail_config = state.config.read().unwrap().mail.clone();
    if verification_required && !mail_config.enabled() {
        return Err(AppError::Internal {
            op: "auth.register.mail_unavailable",
            message: "registration email verification is unavailable".to_owned(),
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

    let registration_grant =
        registration::authorize_registration(&transaction, policy, &input.email, registration_code)
            .await
            .map_err(|error| map_registration_access_error(error, "auth.register.access"))?;

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

    let user = users::create_user(
        &transaction,
        CreateUserParams {
            email: input.email.clone(),
            display_name: display_name.clone(),
            password_hash: Some(password_hash),
            platform_role: registration_platform_role(),
            email_verified_at: (!verification_required).then(time::OffsetDateTime::now_utc),
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

    registration::consume_registration_grant(&transaction, registration_grant, user.id)
        .await
        .map_err(|error| map_registration_access_error(error, "auth.register.consume_access"))?;

    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.register.commit_transaction",
            source: source.into(),
        })?;

    if verification_required {
        let token = authentication::create_auth_token(
            db,
            user.id,
            AuthTokenKind::EmailVerification,
            time::Duration::hours(24),
        )
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.register.verification_token",
            source,
        })?;
        platform_mail::send_email_verification_best_effort(
            db,
            mail_config,
            &user.email,
            &token,
            Some(&return_to),
        )
        .await;
        return Ok(ok_response(json!({
            "verification_required": true,
            "email": user.email,
        }))
        .into_response());
    }

    super::login::authenticated_response(&state, cache, jar, user).await
}

fn registration_platform_role() -> PlatformRole {
    PlatformRole::User
}

pub(crate) fn map_registration_access_error(
    error: registration::RegistrationAccessError,
    op: &'static str,
) -> AppError {
    use crate::domain::codes::CodeUseError;
    use registration::RegistrationAccessError;

    match error {
        RegistrationAccessError::InvalidPolicy => AppError::Internal {
            op,
            message: error.to_string(),
        },
        RegistrationAccessError::Closed | RegistrationAccessError::CredentialRequired => {
            AppError::Forbidden {
                op,
                message: error.to_string(),
            }
        }
        RegistrationAccessError::Code(CodeUseError::NotFound | CodeUseError::WrongScope) => {
            AppError::Forbidden {
                op,
                message: "registration code is invalid".to_owned(),
            }
        }
        RegistrationAccessError::Code(CodeUseError::Used) => AppError::Conflict {
            op,
            message: error.to_string(),
        },
        RegistrationAccessError::Code(CodeUseError::Expired) => AppError::Gone {
            op,
            message: error.to_string(),
        },
        RegistrationAccessError::Code(CodeUseError::Revoked) => AppError::Forbidden {
            op,
            message: error.to_string(),
        },
        RegistrationAccessError::Code(CodeUseError::Database(source))
        | RegistrationAccessError::Database(source) => AppError::Infrastructure { op, source },
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

fn validate_registration_input(email: &str) -> Result<RegistrationInput, AppError> {
    let email = grass_validator::normalize_email(email).map_err(|error| AppError::Validation {
        op: "auth.register.invalid_email",
        message: error.to_string(),
    })?;
    Ok(RegistrationInput { email })
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

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use grass_cache::{CacheBackend, CacheStore};
    use sea_orm::{DbBackend, MockDatabase};
    use time::OffsetDateTime;
    use tower::ServiceExt;
    use uuid::Uuid;

    use super::*;
    use crate::infra::{
        config::ControlApiConfig,
        database::entity::{SystemSettingValueKind, registration_email_allowlist, system_setting},
    };

    fn signup_policy(value: &str) -> system_setting::Model {
        let now = OffsetDateTime::now_utc();
        system_setting::Model {
            id: Uuid::now_v7(),
            key: "signup.policy".to_owned(),
            value_kind: SystemSettingValueKind::String,
            value: serde_json::json!(value),
            is_secret: false,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn legacy_team_invitation_token_cannot_authorize_restricted_registration() {
        for policy in ["invite_only", "closed"] {
            let database = MockDatabase::new(DbBackend::Postgres)
                .append_query_results([Vec::<system_setting::Model>::new()])
                .append_query_results([[signup_policy(policy)]])
                .append_query_results([Vec::<system_setting::Model>::new()])
                .append_query_results([Vec::<registration_email_allowlist::Model>::new()])
                .into_connection();
            let cache = CacheStore::connect_cache(CacheBackend::Moka, "")
                .await
                .unwrap();
            let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
            assert!(state.database.set(database).is_ok());
            assert!(state.cache.set(cache).is_ok());

            let request = Request::builder()
                .method("POST")
                .uri("/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "email": "new-user@example.com",
                        "password": "correct horse battery staple",
                        "display_name": "New User",
                        "invitation_token": "legacy-team-invitation"
                    })
                    .to_string(),
                ))
                .unwrap();
            let response = super::super::router()
                .with_state(state)
                .oneshot(request)
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::FORBIDDEN, "policy {policy}");
        }
    }

    #[test]
    fn registration_request_accepts_a_code_and_local_return_destination() {
        let request: RegisterRequest = serde_json::from_value(serde_json::json!({
            "email": "user@example.com",
            "password": "correct horse battery staple",
            "registration_code": "registration-code",
            "return_to": "/invitations/accept?token=invite-token"
        }))
        .unwrap();

        assert_eq!(
            request.registration_code.as_deref(),
            Some("registration-code")
        );
        assert_eq!(
            request.return_to.as_deref(),
            Some("/invitations/accept?token=invite-token")
        );
    }

    #[test]
    fn registration_access_failures_have_stable_http_meaning() {
        assert!(matches!(
            map_registration_access_error(
                registration::RegistrationAccessError::Closed,
                "auth.register.access",
            ),
            AppError::Forbidden { .. }
        ));
        assert!(matches!(
            map_registration_access_error(
                registration::RegistrationAccessError::CredentialRequired,
                "auth.register.access",
            ),
            AppError::Forbidden { .. }
        ));
        assert!(matches!(
            map_registration_access_error(
                registration::RegistrationAccessError::Code(
                    crate::domain::codes::CodeUseError::Used,
                ),
                "auth.register.access",
            ),
            AppError::Conflict { .. }
        ));
    }

    #[test]
    fn normalizes_and_validates_registration_input() {
        let input = validate_registration_input("  User@Example.COM ").unwrap();
        assert_eq!(input.email, "user@example.com");
        assert!(validate_registration_input("invalid").is_err());
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
