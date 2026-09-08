use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
CREATE TABLE regional_ingresses (
    id UUID PRIMARY KEY,
    region TEXT NOT NULL,
    hostname VARCHAR(253) NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    health_check_path TEXT NOT NULL DEFAULT '/health',
    health_check_interval_seconds INTEGER NOT NULL DEFAULT 30,
    origin_host_preservation BOOLEAN NOT NULL DEFAULT TRUE,
    tls_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    certificate_issuer TEXT NOT NULL DEFAULT 'letsencrypt',
    certificate_auto_renew BOOLEAN NOT NULL DEFAULT TRUE,
    certificate_status TEXT NOT NULL DEFAULT 'pending',
    certificate_expires_at TIMESTAMPTZ NULL,
    certificate_error TEXT NULL,
    dns_challenge_provider TEXT NULL,
    dns_challenge_config JSONB NOT NULL DEFAULT '{}',
    dns_challenge_status TEXT NOT NULL DEFAULT 'not_configured',
    dns_challenge_record_name TEXT NULL,
    dns_challenge_record_value TEXT NULL,
    deleted_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT ck_regional_ingresses_region_nonempty
        CHECK (char_length(btrim(region)) > 0),
    CONSTRAINT ck_regional_ingresses_hostname_nonempty
        CHECK (char_length(btrim(hostname)) > 0),
    CONSTRAINT ck_regional_ingresses_health_path
        CHECK (health_check_path LIKE '/%'),
    CONSTRAINT ck_regional_ingresses_health_interval
        CHECK (health_check_interval_seconds BETWEEN 5 AND 3600),
    CONSTRAINT ck_regional_ingresses_certificate_issuer
        CHECK (certificate_issuer IN ('letsencrypt', 'zerossl', 'manual')),
    CONSTRAINT ck_regional_ingresses_certificate_status
        CHECK (certificate_status IN ('pending', 'issuing', 'active', 'expiring', 'failed', 'disabled')),
    CONSTRAINT ck_regional_ingresses_dns_challenge_status
        CHECK (dns_challenge_status IN ('not_configured', 'pending', 'valid', 'failed'))
);

ALTER TABLE project_host_bindings
    ADD COLUMN region TEXT NOT NULL DEFAULT 'default',
    ADD CONSTRAINT ck_project_host_bindings_region_nonempty
        CHECK (char_length(btrim(region)) > 0);
CREATE INDEX ix_project_host_bindings_region
    ON project_host_bindings (region)
    WHERE deleted_at IS NULL;

ALTER TABLE host_sources
    ADD COLUMN region TEXT NOT NULL DEFAULT 'default',
    ADD CONSTRAINT ck_host_sources_region_nonempty
        CHECK (char_length(btrim(region)) > 0);
CREATE INDEX ix_host_sources_region
    ON host_sources (region)
    WHERE deleted_at IS NULL;

CREATE UNIQUE INDEX ux_regional_ingresses_region_active
    ON regional_ingresses (region)
    WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX ux_regional_ingresses_hostname_active
    ON regional_ingresses (hostname)
    WHERE deleted_at IS NULL;
CREATE INDEX ix_regional_ingresses_enabled
    ON regional_ingresses (region, enabled)
    WHERE deleted_at IS NULL;
"#;

pub(crate) const DOWN_SQL: &str = r#"
DROP INDEX IF EXISTS ix_regional_ingresses_enabled;
DROP INDEX IF EXISTS ux_regional_ingresses_hostname_active;
DROP INDEX IF EXISTS ux_regional_ingresses_region_active;
DROP TABLE regional_ingresses;
DROP INDEX IF EXISTS ix_project_host_bindings_region;
ALTER TABLE project_host_bindings
    DROP CONSTRAINT ck_project_host_bindings_region_nonempty,
    DROP COLUMN region;
DROP INDEX IF EXISTS ix_host_sources_region;
ALTER TABLE host_sources
    DROP CONSTRAINT ck_host_sources_region_nonempty,
    DROP COLUMN region;
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
    fn migration_records_regional_ingress_and_certificate_controls() {
        assert!(UP_SQL.contains("CREATE TABLE regional_ingresses"));
        assert!(UP_SQL.contains("certificate_issuer TEXT NOT NULL DEFAULT 'letsencrypt'"));
        assert!(UP_SQL.contains("certificate_status TEXT NOT NULL DEFAULT 'pending'"));
        assert!(UP_SQL.contains("dns_challenge_config JSONB NOT NULL DEFAULT '{}'"));
        assert!(UP_SQL.contains("dns_challenge_status TEXT NOT NULL DEFAULT 'not_configured'"));
        assert!(UP_SQL.contains("ux_regional_ingresses_region_active"));
        assert!(DOWN_SQL.contains("DROP TABLE regional_ingresses"));
    }
}
