use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::infra::{database::entity::region, error::AppError};

pub async fn list<C: ConnectionTrait>(db: &C) -> anyhow::Result<Vec<region::Model>> {
    Ok(region::Entity::find()
        .order_by_asc(region::Column::Code)
        .all(db)
        .await?)
}

pub async fn require<C: ConnectionTrait>(
    db: &C,
    code: &str,
    op: &'static str,
) -> Result<(), AppError> {
    if region::Entity::find_by_id(code)
        .lock_shared()
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?
        .is_none()
    {
        return Err(AppError::Validation {
            op,
            message: format!(
                "Unknown region '{code}'. Create it in platform settings or with New Region first."
            ),
        });
    }
    Ok(())
}

pub struct AvailableRegion {
    pub code: String,
    pub name: String,
    pub ingress_hostname: Option<String>,
    pub ingress_enabled: bool,
}

pub async fn available<C: ConnectionTrait>(db: &C) -> anyhow::Result<Vec<AvailableRegion>> {
    use crate::infra::database::entity::regional_ingress;
    let entries = regional_ingress::Entity::find()
        .filter(regional_ingress::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    Ok(list(db)
        .await?
        .into_iter()
        .map(|region| {
            let entry = entries.iter().find(|entry| entry.region == region.code);
            AvailableRegion {
                code: region.code,
                name: region.name,
                ingress_hostname: entry.map(|entry| entry.hostname.clone()),
                ingress_enabled: entry.is_some_and(|entry| entry.enabled),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{DbBackend, MockDatabase};
    #[tokio::test]
    async fn unknown_region_requires_explicit_creation() {
        let db = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([Vec::<region::Model>::new()])
            .into_connection();
        assert!(matches!(
            require(&db, "hk_1", "test").await,
            Err(AppError::Validation { .. })
        ));
    }
    #[tokio::test]
    async fn catalog_does_not_hide_regions_without_an_entry() {
        let db = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([
                Vec::<crate::infra::database::entity::regional_ingress::Model>::new(),
            ])
            .append_query_results([vec![region::Model {
                code: "hk_1".into(),
                name: "Hong Kong".into(),
                created_at: time::OffsetDateTime::UNIX_EPOCH,
                updated_at: time::OffsetDateTime::UNIX_EPOCH,
            }]])
            .into_connection();
        let view = available(&db).await.unwrap();
        assert_eq!(view[0].code, "hk_1");
        assert!(!view[0].ingress_enabled);
        assert!(view[0].ingress_hostname.is_none());
    }
}
