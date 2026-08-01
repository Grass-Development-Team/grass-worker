use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
ALTER TABLE deployment_artifacts
    ADD COLUMN deleted_at TIMESTAMPTZ NULL;

CREATE INDEX ix_deployment_artifacts_live
    ON deployment_artifacts (deployment_id, kind, created_at)
    WHERE deleted_at IS NULL;
"#;

pub(crate) const DOWN_SQL: &str = r#"
DROP INDEX ix_deployment_artifacts_live;
ALTER TABLE deployment_artifacts DROP COLUMN deleted_at;
"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(UP_SQL).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(DOWN_SQL)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_adds_a_nullable_tombstone_and_live_index() {
        assert!(UP_SQL.contains("ADD COLUMN deleted_at TIMESTAMPTZ NULL"));
        assert!(UP_SQL.contains("CREATE INDEX ix_deployment_artifacts_live"));
        assert!(UP_SQL.contains("WHERE deleted_at IS NULL"));
        assert!(DOWN_SQL.contains("DROP COLUMN deleted_at"));
    }
}
