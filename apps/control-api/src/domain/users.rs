use std::sync::LazyLock;

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, QueryFilter,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{
    PlatformRole, UserStatus, user, user_password_credential, user_password_history,
};

pub struct CreateUserParams {
    pub email: String,
    pub display_name: Option<String>,
    pub password_hash: Option<String>,
    pub platform_role: PlatformRole,
    pub email_verified_at: Option<OffsetDateTime>,
}

static DUMMY_PASSWORD_HASH: LazyLock<String> = LazyLock::new(|| {
    grass_crypto::hash_password("grass-worker-invalid-credential-placeholder")
        .expect("dummy password hash must be constructible")
});

pub async fn create_user<C: ConnectionTrait>(
    db: &C,
    params: CreateUserParams,
) -> anyhow::Result<user::Model> {
    let now = OffsetDateTime::now_utc();
    let user_id = Uuid::now_v7();

    let user_model = user::ActiveModel {
        id: Set(user_id),
        email: Set(params.email.clone()),
        display_name: Set(params.display_name),
        status: Set(UserStatus::Active),
        platform_role: Set(params.platform_role),
        email_verified_at: Set(params.email_verified_at),
        last_login_at: Set(None),
        deleted_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;

    if let Some(password_hash) = params.password_hash {
        user_password_credential::ActiveModel {
            id: Set(Uuid::now_v7()),
            user_id: Set(user_id),
            password_hash: Set(password_hash.clone()),
            must_change_password: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await?;
        insert_password_history(db, user_id, password_hash, now).await?;
    }

    Ok(user_model)
}

pub async fn any_user_exists<C: ConnectionTrait>(db: &C) -> anyhow::Result<bool> {
    user::Entity::find()
        .filter(user::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map(|o| o.is_some())
        .map_err(|e| anyhow::anyhow!("failed to check users existence: {e}"))
}

pub async fn get_user_by_email<C: ConnectionTrait>(
    db: &C,
    email: &str,
) -> anyhow::Result<Option<user::Model>> {
    user::Entity::find()
        .filter(user::Column::Email.eq(email))
        .filter(user::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(|e| anyhow::anyhow!("failed to query user by email: {e}"))
}

pub async fn get_user_by_id(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> anyhow::Result<Option<user::Model>> {
    user::Entity::find()
        .filter(user::Column::Id.eq(user_id))
        .filter(user::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(|e| anyhow::anyhow!("failed to query user by id: {e}"))
}

pub async fn update_last_login(db: &DatabaseConnection, user_id: Uuid) -> anyhow::Result<()> {
    let now = OffsetDateTime::now_utc();
    user::ActiveModel {
        id: Set(user_id),
        last_login_at: Set(Some(now)),
        ..Default::default()
    }
    .update(db)
    .await?;
    Ok(())
}

pub struct UserListFilter {
    pub query: Option<String>,
    pub status: Option<UserStatus>,
    pub platform_role: Option<PlatformRole>,
    pub limit: u64,
}

/// Platform-wide user listing for administrators, newest first, with an
/// optional case-insensitive email / display name search.
pub async fn list_users<C: ConnectionTrait>(
    db: &C,
    filter: UserListFilter,
) -> anyhow::Result<Vec<user::Model>> {
    use sea_orm::{QueryOrder, QuerySelect};

    let mut query = user::Entity::find().filter(user::Column::DeletedAt.is_null());
    if let Some(status) = filter.status {
        query = query.filter(user::Column::Status.eq(status));
    }
    if let Some(platform_role) = filter.platform_role {
        query = query.filter(user::Column::PlatformRole.eq(platform_role));
    }
    if let Some(term) = filter
        .query
        .as_deref()
        .map(str::trim)
        .filter(|term| !term.is_empty())
    {
        let pattern = format!("%{}%", escape_like(term));
        query = query.filter(
            sea_orm::Condition::any()
                .add(user::Column::Email.like(pattern.clone()))
                .add(user::Column::DisplayName.like(pattern)),
        );
    }
    query
        .order_by_desc(user::Column::CreatedAt)
        .limit(filter.limit.clamp(1, 500))
        .all(db)
        .await
        .map_err(|e| anyhow::anyhow!("failed to list users: {e}"))
}

fn escape_like(term: &str) -> String {
    term.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub struct UpdateUserParams {
    /// `Some(None)` clears the display name.
    pub display_name: Option<Option<String>>,
    pub status: Option<UserStatus>,
    pub platform_role: Option<PlatformRole>,
}

pub async fn update_user<C: ConnectionTrait>(
    db: &C,
    user: user::Model,
    params: UpdateUserParams,
) -> anyhow::Result<user::Model> {
    let mut active: user::ActiveModel = user.into();
    if let Some(display_name) = params.display_name {
        active.display_name = Set(display_name);
    }
    if let Some(status) = params.status {
        active.status = Set(status);
    }
    if let Some(role) = params.platform_role {
        active.platform_role = Set(role);
    }
    active.update(db).await.map_err(Into::into)
}

/// Active platform administrators — the count that must never reach zero.
pub async fn count_active_admins<C: ConnectionTrait>(db: &C) -> anyhow::Result<u64> {
    use sea_orm::PaginatorTrait;

    user::Entity::find()
        .filter(user::Column::DeletedAt.is_null())
        .filter(user::Column::Status.eq(UserStatus::Active))
        .filter(user::Column::PlatformRole.eq(PlatformRole::Admin))
        .count(db)
        .await
        .map_err(|e| anyhow::anyhow!("failed to count platform administrators: {e}"))
}

/// Replaces (or creates) the password credential for a user.
pub async fn set_password<C: ConnectionTrait>(
    db: &C,
    user_id: Uuid,
    password_hash: String,
) -> anyhow::Result<()> {
    let existing = user_password_credential::Entity::find()
        .filter(user_password_credential::Column::UserId.eq(user_id))
        .one(db)
        .await
        .map_err(|e| anyhow::anyhow!("failed to query password credential: {e}"))?;

    match existing {
        Some(credential) => {
            let mut active: user_password_credential::ActiveModel = credential.into();
            active.password_hash = Set(password_hash.clone());
            active.update(db).await?;
        }
        None => {
            let now = OffsetDateTime::now_utc();
            user_password_credential::ActiveModel {
                id: Set(Uuid::now_v7()),
                user_id: Set(user_id),
                password_hash: Set(password_hash.clone()),
                must_change_password: Set(false),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(db)
            .await?;
        }
    }
    insert_password_history(db, user_id, password_hash, OffsetDateTime::now_utc()).await?;
    Ok(())
}

async fn insert_password_history<C: ConnectionTrait>(
    db: &C,
    user_id: Uuid,
    password_hash: String,
    created_at: OffsetDateTime,
) -> anyhow::Result<()> {
    user_password_history::ActiveModel {
        id: Set(Uuid::now_v7()),
        user_id: Set(user_id),
        password_hash: Set(password_hash),
        created_at: Set(created_at),
    }
    .insert(db)
    .await?;
    Ok(())
}

pub async fn verify_user_password(
    db: &DatabaseConnection,
    email: &str,
    password: &str,
) -> anyhow::Result<Option<user::Model>> {
    let user_model = get_user_by_email(db, email).await?;
    let credential = match &user_model {
        Some(user) => user_password_credential::Entity::find()
            .filter(user_password_credential::Column::UserId.eq(user.id))
            .one(db)
            .await
            .map_err(|e| anyhow::anyhow!("failed to query password credential: {e}"))?,
        None => None,
    };
    let password_hash = credential
        .as_ref()
        .map(|credential| credential.password_hash.as_str())
        .unwrap_or(DUMMY_PASSWORD_HASH.as_str());
    let password_ok = grass_crypto::verify_password(password, password_hash).unwrap_or(false);
    let status = user_model.as_ref().map(|user| &user.status);

    if !credentials_are_valid(status, password_ok) || credential.is_none() {
        return Ok(None);
    }

    let user_model = user_model.expect("valid credentials require an existing user");
    Ok(Some(user_model))
}

fn credentials_are_valid(status: Option<&UserStatus>, password_ok: bool) -> bool {
    matches!(status, Some(UserStatus::Active)) && password_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_active_users_with_valid_passwords_can_authenticate() {
        assert!(credentials_are_valid(Some(&UserStatus::Active), true));
        assert!(!credentials_are_valid(Some(&UserStatus::Active), false));
        assert!(!credentials_are_valid(Some(&UserStatus::Disabled), true));
        assert!(!credentials_are_valid(None, true));
    }

    #[tokio::test]
    async fn administrator_user_list_applies_status_and_role_filters() {
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([Vec::<user::Model>::new()])
            .into_connection();
        let log = db.clone();

        list_users(
            &db,
            UserListFilter {
                query: None,
                status: Some(UserStatus::Disabled),
                platform_role: Some(PlatformRole::Admin),
                limit: 25,
            },
        )
        .await
        .unwrap();

        let statements = format!("{:?}", log.into_transaction_log());
        assert!(statements.contains("status\\\" ="), "{statements}");
        assert!(statements.contains("platform_role\\\" ="), "{statements}");
    }
}
