use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, QueryFilter,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{UserStatus, user, user_password_credential};

pub struct CreateUserParams {
    pub email: String,
    pub display_name: Option<String>,
    pub password_hash: String,
}

pub struct VerifyPasswordResult {
    pub user: user::Model,
    pub password_ok: bool,
}

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
        last_login_at: Set(None),
        deleted_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;

    user_password_credential::ActiveModel {
        id: Set(Uuid::now_v7()),
        user_id: Set(user_id),
        password_hash: Set(params.password_hash),
        must_change_password: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;

    Ok(user_model)
}

pub async fn any_user_exists(db: &DatabaseConnection) -> anyhow::Result<bool> {
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

pub async fn verify_user_password(
    db: &DatabaseConnection,
    email: &str,
    password: &str,
) -> anyhow::Result<VerifyPasswordResult> {
    let user_model = get_user_by_email(db, email)
        .await?
        .ok_or_else(|| anyhow::anyhow!("user not found"))?;

    let credential = user_password_credential::Entity::find()
        .filter(user_password_credential::Column::UserId.eq(user_model.id))
        .one(db)
        .await
        .map_err(|e| anyhow::anyhow!("failed to query password credential: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("password credential not found"))?;

    let password_ok =
        grass_crypto::verify_password(password, &credential.password_hash).unwrap_or(false);

    if password_ok {
        update_last_login(db, user_model.id).await?;
    }

    Ok(VerifyPasswordResult {
        password_ok,
        user: if password_ok {
            user_model
        } else {
            // re-fetch to get the updated last_login_at
            get_user_by_id(db, user_model.id)
                .await?
                .unwrap_or(user_model)
        },
    })
}
