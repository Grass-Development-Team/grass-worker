use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
ALTER TABLE nodes
    ADD COLUMN region TEXT NOT NULL DEFAULT 'default',
    ADD CONSTRAINT ck_nodes_region_nonempty CHECK (char_length(btrim(region)) > 0);

ALTER TABLE deployments
    ADD COLUMN region TEXT NOT NULL DEFAULT 'default',
    ADD CONSTRAINT ck_deployments_region_nonempty CHECK (char_length(btrim(region)) > 0);

CREATE INDEX ix_nodes_region_health
    ON nodes (region, status, last_heartbeat_at)
    WHERE deleted_at IS NULL AND serve_enabled = TRUE;

CREATE INDEX ix_deployments_region_status
    ON deployments (region, serve_status)
    WHERE deleted_at IS NULL;
"#;

pub(crate) const DOWN_SQL: &str = r#"
DROP INDEX IF EXISTS ix_deployments_region_status;
DROP INDEX IF EXISTS ix_nodes_region_health;
ALTER TABLE deployments DROP CONSTRAINT ck_deployments_region_nonempty, DROP COLUMN region;
ALTER TABLE nodes DROP CONSTRAINT ck_nodes_region_nonempty, DROP COLUMN region;
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
    fn migration_adds_defaulted_regions_and_indexes() {
        assert!(UP_SQL.contains("ADD COLUMN region TEXT NOT NULL DEFAULT 'default'"));
        assert!(UP_SQL.contains("ix_nodes_region_health"));
        assert!(UP_SQL.contains("ix_deployments_region_status"));
        assert!(DOWN_SQL.contains("DROP COLUMN region"));
    }
}
