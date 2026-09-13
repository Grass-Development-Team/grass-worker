use crate::infra::database::entity::{PlatformRole, UserStatus, user};

pub(crate) fn active_user() -> user::Model {
    let now = time::OffsetDateTime::now_utc();
    user::Model {
        id: uuid::Uuid::now_v7(),
        email: "session@example.test".into(),
        display_name: None,
        avatar_version: None,
        auth_version: 1,
        status: UserStatus::Active,
        platform_role: PlatformRole::User,
        email_verified_at: Some(now),
        last_login_at: None,
        deleted_at: None,
        created_at: now,
        updated_at: now,
    }
}
