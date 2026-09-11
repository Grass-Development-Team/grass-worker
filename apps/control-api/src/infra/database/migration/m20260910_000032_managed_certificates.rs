use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
ALTER TABLE regional_ingresses ALTER COLUMN health_check_path SET DEFAULT '/_grass/health';
UPDATE regional_ingresses SET health_check_path = '/_grass/health' WHERE health_check_path = '/health';
CREATE TABLE managed_certificates (
    id UUID PRIMARY KEY,
    ingress_id UUID NOT NULL REFERENCES regional_ingresses(id) ON DELETE CASCADE,
    host_binding_id UUID NULL UNIQUE REFERENCES project_host_bindings(id) ON DELETE CASCADE,
    hostname VARCHAR(253) NOT NULL,
    issuer TEXT NOT NULL CHECK (issuer IN ('letsencrypt', 'zerossl', 'manual')),
    challenge_method TEXT NOT NULL DEFAULT 'http01' CHECK (challenge_method IN ('http01', 'dns01')),
    auto_renew BOOLEAN NOT NULL DEFAULT TRUE,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','issuing','active','failed','disabled')),
    error TEXT NULL,
    bundle JSONB NULL,
    acme_account JSONB NULL,
    revision TEXT NOT NULL DEFAULT '',
    issued_at TIMESTAMPTZ NULL,
    expires_at TIMESTAMPTZ NULL,
    retry_at TIMESTAMPTZ NULL,
    failure_count INTEGER NOT NULL DEFAULT 0 CHECK (failure_count >= 0),
    lease_until TIMESTAMPTZ NULL,
    generation UUID NOT NULL,
    challenge_token TEXT NULL,
    challenge_value TEXT NULL,
    challenge_expires_at TIMESTAMPTZ NULL,
    dns_record_name TEXT NULL,
    dns_record_value TEXT NULL,
    dns_cleanup JSONB NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((challenge_token IS NULL AND challenge_value IS NULL AND challenge_expires_at IS NULL)
        OR (challenge_token IS NOT NULL AND challenge_value IS NOT NULL AND challenge_expires_at IS NOT NULL))
);
CREATE UNIQUE INDEX ux_managed_certificates_regional ON managed_certificates(ingress_id) WHERE host_binding_id IS NULL;
CREATE INDEX ix_managed_certificates_retry ON managed_certificates(retry_at, expires_at);
CREATE TABLE node_ingress_status (
    node_id UUID PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
    certificates JSONB NOT NULL DEFAULT '[]',
    challenge_revision TEXT NOT NULL DEFAULT '',
    tls_ready BOOLEAN NOT NULL DEFAULT FALSE,
    checked_at TIMESTAMPTZ NOT NULL
);
"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(UP_SQL).await?;
        Ok(())
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared("DROP TABLE node_ingress_status; DROP TABLE managed_certificates; ALTER TABLE regional_ingresses ALTER COLUMN health_check_path SET DEFAULT '/health';").await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, ensure};
    use sea_orm::{Database, DatabaseBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    /// Read-only inspection of an already migrated, operator-provided database.
    #[tokio::test]
    #[ignore = "requires GRASS_TEST_DATABASE_URL pointing to a database with all current migrations applied"]
    async fn managed_certificate_schema_matches_signed_lifecycle_and_ack_protocol()
    -> anyhow::Result<()> {
        let url = std::env::var("GRASS_TEST_DATABASE_URL")
            .context("test database runtime configuration required")?;
        let db = Database::connect(url).await?;
        let applied =
            crate::infra::database::migrate::Migrator::get_applied_migrations(&db).await?;
        ensure!(
            applied
                .iter()
                .any(|m| m.name() == "m20260910_000032_managed_certificates"),
            "managed certificate migration was not recorded"
        );
        ensure!(
            crate::infra::database::migrate::Migrator::get_pending_migrations(&db)
                .await?
                .is_empty(),
            "pending migrations remain"
        );
        let columns=db.query_all_raw(Statement::from_string(DatabaseBackend::Postgres,"SELECT table_name,column_name,udt_name,is_nullable,column_default FROM information_schema.columns WHERE table_schema=current_schema() AND table_name IN ('managed_certificates','node_ingress_status')")).await?;
        let mut found = std::collections::BTreeMap::new();
        for column in columns {
            let table: String = column.try_get("", "table_name")?;
            let name: String = column.try_get("", "column_name")?;
            found.insert(
                (table, name),
                (
                    column.try_get::<String>("", "udt_name")?,
                    column.try_get::<String>("", "is_nullable")?,
                    column.try_get::<Option<String>>("", "column_default")?,
                ),
            );
        }
        for name in [
            "issued_at",
            "expires_at",
            "retry_at",
            "lease_until",
            "challenge_expires_at",
        ] {
            ensure!(
                found.get(&("managed_certificates".to_owned(), name.to_owned()))
                    == Some(&("timestamptz".to_owned(), "YES".to_owned(), None)),
                "nullable lifecycle timestamp {name} has wrong type or default"
            );
        }
        for (table, name, kind, nullable) in [
            ("managed_certificates", "bundle", "jsonb", "YES"),
            ("managed_certificates", "contact_email", "text", "NO"),
            ("managed_certificates", "generation", "uuid", "NO"),
            ("managed_certificates", "host_binding_id", "uuid", "NO"),
            ("node_ingress_status", "certificates", "jsonb", "NO"),
            ("node_ingress_status", "tls_ready", "bool", "NO"),
            ("node_ingress_status", "checked_at", "timestamptz", "NO"),
        ] {
            let column = found
                .get(&(table.to_owned(), name.to_owned()))
                .context("required certificate column missing")?;
            ensure!(
                column.0 == kind && column.1 == nullable,
                "certificate column shape mismatch"
            );
        }
        let indexes=db.query_all_raw(Statement::from_string(DatabaseBackend::Postgres,"SELECT indexdef FROM pg_indexes WHERE schemaname=current_schema() AND tablename='managed_certificates'")).await?;
        let indexes = indexes
            .iter()
            .map(|r| r.try_get::<String>("", "indexdef"))
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        ensure!(
            indexes.contains("UNIQUE INDEX managed_certificates_host_binding_id_key")
                && !indexes.contains("ux_managed_certificates_regional")
                && indexes.contains("ix_managed_certificates_retry"),
            "certificate uniqueness/retry indexes missing"
        );
        let constraints=db.query_all_raw(Statement::from_string(DatabaseBackend::Postgres,"SELECT pg_get_constraintdef(oid) AS definition FROM pg_constraint WHERE conrelid IN ('managed_certificates'::regclass,'node_ingress_status'::regclass)")).await?;
        let constraints = constraints
            .iter()
            .map(|r| r.try_get::<String>("", "definition"))
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        for required in [
            "REFERENCES regional_ingresses(id)",
            "REFERENCES project_host_bindings(id)",
            "REFERENCES nodes(id)",
            "http01",
            "letsencrypt",
            "zerossl",
            "manual",
            "failure_count >= 0",
            "challenge_expires_at IS NOT NULL",
        ] {
            ensure!(
                constraints.contains(required),
                "certificate constraint missing: {required}"
            );
        }
        db.close().await?;
        Ok(())
    }
}
