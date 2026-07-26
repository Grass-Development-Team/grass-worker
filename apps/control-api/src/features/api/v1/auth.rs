pub mod csrf_token;
pub mod login;
pub mod logout;
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
        .route("/logout", post(logout::handler))
        .route("/csrf", get(csrf_token::handler))
}

pub(crate) fn user_data(user: &user::Model) -> Value {
    json!({
        "id": user.id,
        "email": user.email,
        "display_name": user.display_name,
        "platform_role": user.platform_role.as_str(),
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
        let user = user::Model {
            id: Uuid::nil(),
            email: "admin@example.com".to_owned(),
            display_name: Some("Admin".to_owned()),
            status: UserStatus::Active,
            platform_role: PlatformRole::Admin,
            last_login_at: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };

        assert_eq!(user_data(&user)["platform_role"], "admin");
    }
}
