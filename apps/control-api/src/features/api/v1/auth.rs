pub mod csrf_token;
pub mod email_verification;
pub mod login;
pub mod logout;
pub mod mfa;
pub mod oidc;
pub mod password;
pub mod register;

use axum::{
    Router,
    routing::{get, post},
};
use serde_json::{Value, json};

use crate::{infra::database::entity::user, state::ControlApiState};

pub fn router() -> Router<ControlApiState> {
    Router::new()
        .route("/login", post(login::handler))
        .route("/register", post(register::handler))
        .route("/email/verify", post(email_verification::verify))
        .route("/email/resend", post(email_verification::resend))
        .route("/password/forgot", post(password::forgot))
        .route("/password/reset", post(password::reset))
        .route("/mfa/challenge", post(mfa::challenge_status))
        .route("/mfa/totp/start", post(mfa::challenge_totp_start))
        .route("/mfa/email/send", post(mfa::challenge_email_send))
        .route("/mfa/verify", post(mfa::challenge_verify))
        .route("/providers", get(oidc::providers))
        .route("/providers/{slug}/start", get(oidc::start))
        .route(
            "/providers/{slug}/callback",
            get(oidc::callback).post(oidc::callback_form),
        )
        .route("/logout", post(logout::handler))
        .route("/csrf", get(csrf_token::handler))
}

pub(crate) fn user_data(user: &user::Model) -> Value {
    json!({
        "id": user.id,
        "email": user.email,
        "display_name": user.display_name,
        "avatar_url": super::avatars::user_avatar_url(user.id, user.avatar_version),
        "platform_role": user.platform_role.as_str(),
        "email_verified": user.email_verified_at.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;
    use uuid::Uuid;

    use crate::infra::database::entity::{PlatformRole, UserStatus, user};

    use super::user_data;

    #[test]
    fn authenticated_user_data_exposes_the_platform_role() {
        let now = OffsetDateTime::UNIX_EPOCH;
        let avatar_version = Uuid::max();
        let user = user::Model {
            id: Uuid::nil(),
            email: "admin@example.com".to_owned(),
            display_name: Some("Admin".to_owned()),
            avatar_version: Some(avatar_version),
            status: UserStatus::Active,
            platform_role: PlatformRole::Admin,
            email_verified_at: Some(now),
            last_login_at: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };

        assert_eq!(user_data(&user)["platform_role"], "admin");
        assert_eq!(
            user_data(&user)["avatar_url"],
            format!(
                "/api/v1/avatars/users/{}/{avatar_version}/avatar.webp",
                Uuid::nil()
            )
        );
    }
}
