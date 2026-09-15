use super::super::{MIGRATION_TEST_LOCK, Migrator};
use super::support::{PostgresMigrationDatabase, assert_migration_tracking};
use anyhow::{Context, ensure};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;
use std::collections::BTreeMap;

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL"]
async fn postgres_node_deletion_queue_schema_matches_domain_and_is_reversible() -> anyhow::Result<()>
{
    let _migration_guard = MIGRATION_TEST_LOCK.lock().await;
    let database_url = std::env::var("GRASS_TEST_DATABASE_URL")
        .expect("GRASS_TEST_DATABASE_URL must be set to run this ignored migration test");
    let test_db = PostgresMigrationDatabase::start(&database_url).await?;

    let verification = async {
        Migrator::up(&test_db.db, Some(15)).await?;
        assert_migration_tracking(&test_db.db, 15).await?;
        assert_node_deletion_schema(&test_db.db).await?;

        Migrator::down(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 14).await?;
        assert_node_deletion_schema_absent(&test_db.db).await?;

        Migrator::up(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 15).await?;
        assert_node_deletion_schema(&test_db.db).await
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

async fn assert_node_deletion_schema(db: &DatabaseConnection) -> anyhow::Result<()> {
    let enums = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT t.typname, string_agg(e.enumlabel, ',' ORDER BY e.enumsortorder) AS labels
FROM pg_type t
JOIN pg_enum e ON e.enumtypid = t.oid
JOIN pg_namespace n ON n.oid = t.typnamespace
WHERE n.nspname = current_schema()
  AND t.typname IN ('node_deletion_status', 'node_deployment_migration_status')
GROUP BY t.typname
ORDER BY t.typname
"#,
        ))
        .await?
        .into_iter()
        .map(|row| {
            Ok((
                row.try_get::<String>("", "typname")?,
                row.try_get::<String>("", "labels")?,
            ))
        })
        .collect::<Result<Vec<_>, sea_orm::DbErr>>()?;
    ensure!(
        enums
            == vec![
                (
                    "node_deletion_status".to_owned(),
                    "queued,migrating,draining,deleting,failed,completed".to_owned(),
                ),
                (
                    "node_deployment_migration_status".to_owned(),
                    "pending,syncing,ready,failed".to_owned(),
                ),
            ],
        "node deletion enum values did not match the domain model: {enums:?}"
    );

    let columns = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT table_name, column_name, udt_name, is_nullable, column_default
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND table_name IN ('node_deletion_jobs', 'node_deployment_migrations')
ORDER BY table_name, ordinal_position
"#,
        ))
        .await?;
    ensure!(
        columns.len() == 22,
        "expected 22 node deletion columns, found {}",
        columns.len()
    );
    let shapes = columns
        .into_iter()
        .map(|row| {
            Ok((
                format!(
                    "{}.{}",
                    row.try_get::<String>("", "table_name")?,
                    row.try_get::<String>("", "column_name")?,
                ),
                (
                    row.try_get::<String>("", "udt_name")?,
                    row.try_get::<String>("", "is_nullable")?,
                    row.try_get::<Option<String>>("", "column_default")?,
                ),
            ))
        })
        .collect::<Result<BTreeMap<_, _>, sea_orm::DbErr>>()?;
    ensure!(
        shapes.get("node_deletion_jobs.status")
            == Some(&(
                "node_deletion_status".to_owned(),
                "NO".to_owned(),
                Some("'queued'::node_deletion_status".to_owned()),
            ))
    );
    ensure!(
        shapes.get("node_deletion_jobs.completed_at")
            == Some(&("timestamptz".to_owned(), "YES".to_owned(), None))
    );
    ensure!(
        shapes.get("node_deployment_migrations.status")
            == Some(&(
                "node_deployment_migration_status".to_owned(),
                "NO".to_owned(),
                Some("'pending'::node_deployment_migration_status".to_owned()),
            ))
    );
    ensure!(
        shapes.get("node_deployment_migrations.ready_at")
            == Some(&("timestamptz".to_owned(), "YES".to_owned(), None))
    );

    let constraints = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT conname, pg_get_constraintdef(oid) AS definition
FROM pg_constraint
WHERE conrelid IN ('node_deletion_jobs'::regclass, 'node_deployment_migrations'::regclass)
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
    for expected in [
        "ck_node_deletion_jobs_distinct_target",
        "ck_node_deletion_jobs_progress_nonnegative",
        "ck_node_deletion_jobs_progress_bounded",
        "ck_node_deletion_jobs_completed_at",
        "ck_node_deployment_migrations_distinct_nodes",
        "ck_node_deployment_migrations_ready_at",
        "ux_node_deployment_migrations_job_deployment",
    ] {
        ensure!(
            constraints.contains_key(expected),
            "missing constraint {expected}"
        );
    }
    ensure!(
        constraints.values().any(
            |definition| definition.contains("FOREIGN KEY (target_node_id)")
                && definition.contains("REFERENCES nodes(id) ON DELETE RESTRICT")
        ),
        "target Node foreign keys must prevent deleting an active migration target"
    );
    ensure!(
        constraints.values().any(|definition| definition
            .contains("FOREIGN KEY (requested_by_user_id)")
            && definition.contains("ON DELETE SET NULL")),
        "requester foreign key must preserve deletion history"
    );

    let indexes = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT indexname, indexdef
FROM pg_indexes
WHERE schemaname = current_schema()
  AND indexname IN (
'ux_node_deletion_jobs_active_node',
'ix_node_deletion_jobs_queue',
'ix_node_deployment_migrations_target'
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
    ensure!(indexes.len() == 3, "expected three queue indexes");
    ensure!(
        indexes["ux_node_deletion_jobs_active_node"].contains("UNIQUE INDEX")
            && indexes["ux_node_deletion_jobs_active_node"]
                .contains("status <> 'completed'::node_deletion_status")
    );
    ensure!(
        indexes["ix_node_deployment_migrations_target"]
            .contains("'ready'::node_deployment_migration_status")
    );
    Ok(())
}

async fn assert_node_deletion_schema_absent(db: &DatabaseConnection) -> anyhow::Result<()> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT
  to_regclass('node_deletion_jobs') IS NULL AS jobs_absent,
  to_regclass('node_deployment_migrations') IS NULL AS migrations_absent,
  NOT EXISTS (
SELECT 1 FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace
WHERE n.nspname = current_schema()
  AND t.typname IN ('node_deletion_status', 'node_deployment_migration_status')
  ) AS enums_absent
"#,
        ))
        .await?
        .context("node deletion absence query returned no row")?;
    ensure!(row.try_get::<bool>("", "jobs_absent")?);
    ensure!(row.try_get::<bool>("", "migrations_absent")?);
    ensure!(row.try_get::<bool>("", "enums_absent")?);
    Ok(())
}
