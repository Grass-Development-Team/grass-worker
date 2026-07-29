use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
CREATE TYPE node_config_sync_status AS ENUM ('pending', 'applying', 'applied', 'failed');

ALTER TABLE nodes
    ADD COLUMN desired_config JSONB NULL,
    ADD COLUMN desired_config_revision BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN effective_config JSONB NULL,
    ADD COLUMN effective_config_revision BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN config_sync_status node_config_sync_status NOT NULL DEFAULT 'pending',
    ADD COLUMN config_sync_error TEXT NULL,
    ADD COLUMN node_token_configured BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN config_updated_at TIMESTAMPTZ NULL,
    ADD COLUMN config_applied_at TIMESTAMPTZ NULL,
    ADD CONSTRAINT ck_nodes_desired_config_revision_nonnegative
        CHECK (desired_config_revision >= 0),
    ADD CONSTRAINT ck_nodes_effective_config_revision_nonnegative
        CHECK (effective_config_revision >= 0),
    ADD CONSTRAINT ck_nodes_desired_config_object
        CHECK (desired_config IS NULL OR jsonb_typeof(desired_config) = 'object'),
    ADD CONSTRAINT ck_nodes_effective_config_object
        CHECK (effective_config IS NULL OR jsonb_typeof(effective_config) = 'object');

CREATE INDEX ix_nodes_config_sync_status
    ON nodes (config_sync_status, updated_at DESC)
    WHERE deleted_at IS NULL AND config_sync_status <> 'applied';
"#;

pub(crate) const DOWN_SQL: &str = r#"
DROP INDEX IF EXISTS ix_nodes_config_sync_status;

ALTER TABLE nodes
    DROP COLUMN config_applied_at,
    DROP COLUMN config_updated_at,
    DROP COLUMN node_token_configured,
    DROP COLUMN config_sync_error,
    DROP COLUMN config_sync_status,
    DROP COLUMN effective_config_revision,
    DROP COLUMN effective_config,
    DROP COLUMN desired_config_revision,
    DROP COLUMN desired_config;

DROP TYPE node_config_sync_status;
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
    fn migration_adds_reversible_node_config_sync_state() {
        assert!(
            UP_SQL.contains(
                "CREATE TYPE node_config_sync_status AS ENUM ('pending', 'applying', 'applied', 'failed')"
            )
        );
        assert!(UP_SQL.contains("ADD COLUMN desired_config JSONB NULL"));
        assert!(UP_SQL.contains("ADD COLUMN desired_config_revision BIGINT NOT NULL DEFAULT 0"));
        assert!(UP_SQL.contains("ADD COLUMN effective_config JSONB NULL"));
        assert!(UP_SQL.contains("ADD COLUMN effective_config_revision BIGINT NOT NULL DEFAULT 0"));
        assert!(UP_SQL.contains("ADD COLUMN config_sync_status node_config_sync_status"));
        assert!(UP_SQL.contains("ADD COLUMN config_sync_error TEXT NULL"));
        assert!(UP_SQL.contains("ADD COLUMN node_token_configured BOOLEAN NOT NULL DEFAULT false"));
        assert!(UP_SQL.contains("ADD COLUMN config_updated_at TIMESTAMPTZ NULL"));
        assert!(UP_SQL.contains("ADD COLUMN config_applied_at TIMESTAMPTZ NULL"));
        assert!(UP_SQL.contains("ck_nodes_desired_config_object"));
        assert!(UP_SQL.contains("ck_nodes_effective_config_object"));
        assert!(DOWN_SQL.contains("DROP TYPE node_config_sync_status"));
    }
}
