use std::collections::BTreeMap;

use anyhow::ensure;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;

use super::super::{MIGRATION_TEST_LOCK, Migrator};
use super::support::{
    PostgresMigrationDatabase, assert_migration_tracking, column, object_count, query_column_shapes,
};

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL"]
async fn postgres_regional_ingress_schema_matches_domain_and_is_reversible() -> anyhow::Result<()> {
    let _migration_guard = MIGRATION_TEST_LOCK.lock().await;
    let database_url = std::env::var("GRASS_TEST_DATABASE_URL")
        .expect("GRASS_TEST_DATABASE_URL must be set to run this ignored migration test");
    let test_db = PostgresMigrationDatabase::start(&database_url).await?;

    let verification = async {
        Migrator::up(&test_db.db, Some(31)).await?;
        assert_migration_tracking(&test_db.db, 31).await?;
        assert_regional_ingress_schema(&test_db.db).await?;

        Migrator::down(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 30).await?;
        assert_regional_ingress_lifecycle_absent(&test_db.db).await?;

        Migrator::down(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 29).await?;
        assert_regional_ingress_schema_absent(&test_db.db).await?;

        Migrator::up(&test_db.db, Some(2)).await?;
        assert_migration_tracking(&test_db.db, 31).await?;
        assert_regional_ingress_schema(&test_db.db).await
    }
    .await;
    let cleanup = test_db.cleanup().await;

    match (verification, cleanup) {
        (Err(verification_error), Err(cleanup_error)) => Err(verification_error.context(format!(
            "disposable schema cleanup also failed: {cleanup_error:#}"
        ))),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

async fn assert_regional_ingress_schema(db: &DatabaseConnection) -> anyhow::Result<()> {
    let columns = query_column_shapes(
        db,
        r#"
SELECT column_name, udt_name, is_nullable, column_default
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND table_name = 'regional_ingresses'
ORDER BY ordinal_position
"#,
    )
    .await?;
    ensure!(
        columns
            == vec![
                column("id", "uuid", "NO", None),
                column("region", "text", "NO", None),
                column("hostname", "varchar", "NO", None),
                column("enabled", "bool", "NO", Some("true")),
                column("health_check_path", "text", "NO", Some("'/health'::text")),
                column("health_check_interval_seconds", "int4", "NO", Some("30")),
                column("origin_host_preservation", "bool", "NO", Some("true")),
                column("tls_enabled", "bool", "NO", Some("true")),
                column(
                    "certificate_issuer",
                    "text",
                    "NO",
                    Some("'letsencrypt'::text")
                ),
                column("certificate_auto_renew", "bool", "NO", Some("true")),
                column("certificate_status", "text", "NO", Some("'pending'::text")),
                column("certificate_expires_at", "timestamptz", "YES", None),
                column("certificate_error", "text", "YES", None),
                column("dns_challenge_provider", "text", "YES", None),
                column("dns_challenge_config", "jsonb", "NO", Some("'{}'::jsonb")),
                column(
                    "dns_challenge_status",
                    "text",
                    "NO",
                    Some("'not_configured'::text")
                ),
                column("dns_challenge_record_name", "text", "YES", None),
                column("dns_challenge_record_value", "text", "YES", None),
                column("deleted_at", "timestamptz", "YES", None),
                column("created_at", "timestamptz", "NO", None),
                column("updated_at", "timestamptz", "NO", None),
                column("acme_account", "jsonb", "YES", None),
                column("certificate_bundle", "jsonb", "YES", None),
                column("certificate_issued_at", "timestamptz", "YES", None),
            ],
        "unexpected regional_ingresses column shapes: {columns:#?}"
    );

    let constraints = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT conname, pg_get_constraintdef(oid) AS definition
FROM pg_constraint
WHERE conrelid = 'regional_ingresses'::regclass
  AND conname LIKE 'ck_regional_ingresses_%'
ORDER BY conname
"#,
        ))
        .await?
        .into_iter()
        .map(|row| {
            Ok((
                row.try_get::<String>("", "conname")?,
                row.try_get::<String>("", "definition")?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, sea_orm::DbErr>>()?;
    ensure!(
        constraints.len() == 7,
        "unexpected ingress constraints: {constraints:#?}"
    );
    ensure!(constraints["ck_regional_ingresses_region_nonempty"].contains("char_length"));
    ensure!(constraints["ck_regional_ingresses_hostname_nonempty"].contains("char_length"));
    // PostgreSQL deparses LIKE and BETWEEN into their underlying operators.
    ensure!(
        constraints["ck_regional_ingresses_health_path"]
            .contains("health_check_path ~~ '/%'::text")
    );
    ensure!(
        constraints["ck_regional_ingresses_health_interval"]
            .contains("health_check_interval_seconds >= 5")
            && constraints["ck_regional_ingresses_health_interval"]
                .contains("health_check_interval_seconds <= 3600")
    );
    ensure!(constraints["ck_regional_ingresses_certificate_issuer"].contains("letsencrypt"));
    ensure!(constraints["ck_regional_ingresses_certificate_status"].contains("certificate_status"));
    ensure!(constraints["ck_regional_ingresses_dns_challenge_status"].contains("not_configured"));

    let indexes = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT indexname, indexdef
FROM pg_indexes
WHERE schemaname = current_schema()
  AND indexname IN (
'ux_regional_ingresses_region_active',
'ux_regional_ingresses_hostname_active',
'ix_regional_ingresses_enabled',
'ix_host_sources_region',
'ix_project_host_bindings_region'
  )
ORDER BY indexname
"#,
        ))
        .await?
        .into_iter()
        .map(|row| {
            Ok((
                row.try_get::<String>("", "indexname")?,
                row.try_get::<String>("", "indexdef")?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, sea_orm::DbErr>>()?;
    ensure!(
        indexes.len() == 5,
        "unexpected ingress indexes: {indexes:#?}"
    );
    ensure!(indexes["ux_regional_ingresses_region_active"].contains("UNIQUE"));
    ensure!(indexes["ux_regional_ingresses_region_active"].contains("deleted_at IS NULL"));
    ensure!(indexes["ux_regional_ingresses_hostname_active"].contains("UNIQUE"));
    ensure!(indexes["ix_regional_ingresses_enabled"].contains("(region, enabled)"));
    ensure!(indexes["ix_host_sources_region"].contains("(region)"));
    ensure!(indexes["ix_project_host_bindings_region"].contains("(region)"));
    ensure!(
        object_count(
            db,
            "SELECT count(*)::bigint AS count FROM information_schema.tables WHERE \
             table_schema = current_schema() AND table_name = 'regional_ingress_health'",
        )
        .await?
            == 1,
        "regional ingress health table is missing"
    );
    Ok(())
}

pub(super) async fn assert_regional_ingress_lifecycle_absent(
    db: &DatabaseConnection,
) -> anyhow::Result<()> {
    ensure!(
        object_count(
            db,
            "SELECT count(*)::bigint AS count FROM information_schema.tables WHERE \
             table_schema = current_schema() AND table_name = 'regional_ingress_health'",
        )
        .await?
            == 0,
        "regional ingress health table remained after lifecycle down migration"
    );
    ensure!(
        object_count(
            db,
            "SELECT count(*)::bigint AS count FROM information_schema.columns WHERE \
             table_schema = current_schema() AND table_name = 'regional_ingresses' AND \
             column_name IN ('acme_account', 'certificate_bundle', \
             'certificate_issued_at')",
        )
        .await?
            == 0,
        "regional ingress lifecycle columns remained after lifecycle down migration"
    );
    Ok(())
}

pub(super) async fn assert_regional_ingress_schema_absent(
    db: &DatabaseConnection,
) -> anyhow::Result<()> {
    ensure!(
        object_count(
            db,
            r#"
SELECT count(*)::bigint AS count
FROM information_schema.tables
WHERE table_schema = current_schema()
  AND table_name = 'regional_ingresses'
"#,
        )
        .await?
            == 0,
        "regional_ingresses table remained after down migration"
    );
    ensure!(
        object_count(
            db,
            r#"
SELECT count(*)::bigint AS count
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND table_name IN ('host_sources', 'project_host_bindings')
  AND column_name = 'region'
"#,
        )
        .await?
            == 0,
        "regional host columns remained after down migration"
    );
    ensure!(
        object_count(
            db,
            r#"
SELECT count(*)::bigint AS count
FROM pg_indexes
WHERE schemaname = current_schema()
  AND indexname IN (
'ux_regional_ingresses_region_active',
'ux_regional_ingresses_hostname_active',
'ix_regional_ingresses_enabled',
'ix_host_sources_region',
'ix_project_host_bindings_region'
  )
"#,
        )
        .await?
            == 0,
        "regional ingress indexes remained after down migration"
    );
    Ok(())
}
