use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{UserStatus, user, user_password_credential};

pub struct CreateUserParams {
    pub email: String,
    pub display_name: Option<String>,
    pub password_hash: String,
}

pub async fn create_user(
    db: &DatabaseConnection,
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
