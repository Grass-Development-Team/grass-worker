use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
ALTER TABLE project_host_bindings
    ADD COLUMN ownership_status TEXT NOT NULL DEFAULT 'not_required',
    ADD COLUMN ownership_checked_at TIMESTAMPTZ NULL,
    ADD COLUMN ownership_error TEXT NULL;

UPDATE project_host_bindings
SET ownership_status = 'pending'
WHERE host_source_id IS NULL AND deleted_at IS NULL;

ALTER TABLE project_host_bindings
    ADD CONSTRAINT ck_project_host_bindings_ownership_status
        CHECK (ownership_status IN ('pending', 'verified', 'failed', 'not_required'));

ALTER TABLE regional_ingresses
    ADD COLUMN acme_account JSONB NULL,
    ADD COLUMN certificate_bundle JSONB NULL,
    ADD COLUMN certificate_issued_at TIMESTAMPTZ NULL;

CREATE TABLE regional_ingress_health (
    ingress_id UUID NOT NULL REFERENCES regional_ingresses(id) ON DELETE CASCADE,
    node_id UUID NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'unknown',
    checked_at TIMESTAMPTZ NULL,
    latency_ms INTEGER NULL,
    error TEXT NULL,
    PRIMARY KEY (ingress_id, node_id),
    CONSTRAINT ck_regional_ingress_health_status
        CHECK (status IN ('unknown', 'healthy', 'unhealthy')),
    CONSTRAINT ck_regional_ingress_health_latency
        CHECK (latency_ms IS NULL OR latency_ms >= 0)
);
CREATE INDEX ix_regional_ingress_health_lookup
    ON regional_ingress_health (ingress_id, status, checked_at);
"#;

pub(crate) const DOWN_SQL: &str = r#"
DROP INDEX IF EXISTS ix_regional_ingress_health_lookup;
DROP TABLE regional_ingress_health;
ALTER TABLE regional_ingresses
    DROP COLUMN certificate_issued_at,
    DROP COLUMN certificate_bundle,
    DROP COLUMN acme_account;
ALTER TABLE project_host_bindings
    DROP CONSTRAINT ck_project_host_bindings_ownership_status,
    DROP COLUMN ownership_error,
    DROP COLUMN ownership_checked_at,
    DROP COLUMN ownership_status;
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
    fn migration_adds_lifecycle_state_and_health_table() {
        assert!(UP_SQL.contains("ownership_status TEXT NOT NULL"));
        assert!(UP_SQL.contains("CREATE TABLE regional_ingress_health"));
        assert!(UP_SQL.contains("certificate_bundle JSONB"));
        assert!(DOWN_SQL.contains("DROP TABLE regional_ingress_health"));
    }
}
