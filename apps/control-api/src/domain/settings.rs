use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{SystemSettingValueKind, system_setting};

pub async fn get_setting(
    db: &DatabaseConnection,
    key: &str,
) -> anyhow::Result<Option<system_setting::Model>> {
    system_setting::Entity::find()
        .filter(system_setting::Column::Key.eq(key))
        .one(db)
        .await
        .map_err(|e| anyhow::anyhow!("failed to read system setting {key}: {e}"))
}

pub async fn set_setting(
    db: &DatabaseConnection,
    key: &str,
    value_kind: SystemSettingValueKind,
    value: serde_json::Value,
) -> anyhow::Result<()> {
    let now = OffsetDateTime::now_utc();
    let existing = get_setting(db, key).await?;

    if let Some(model) = existing {
        let mut active: system_setting::ActiveModel = model.into();
        active.value = Set(value);
        active.value_kind = Set(value_kind);
        active.update(db).await?;
    } else {
        system_setting::ActiveModel {
            id: Set(Uuid::now_v7()),
            key: Set(key.to_owned()),
            value_kind: Set(value_kind),
            value: Set(value),
            is_secret: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await?;
    }

    Ok(())
}

pub async fn set_string(db: &DatabaseConnection, key: &str, value: &str) -> anyhow::Result<()> {
    set_setting(db, key, SystemSettingValueKind::String, json!(value)).await
}
