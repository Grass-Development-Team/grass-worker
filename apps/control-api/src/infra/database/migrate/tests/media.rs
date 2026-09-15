use std::collections::BTreeMap;

use anyhow::{Context, ensure};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;

use super::super::{MIGRATION_TEST_LOCK, Migrator};
use super::ingress::{
    assert_regional_ingress_lifecycle_absent, assert_regional_ingress_schema_absent,
};
use super::support::{
    PostgresMigrationDatabase, assert_migration_tracking, column, object_count, query_column_shapes,
};

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL"]
async fn postgres_media_schema_matches_domain_and_is_reversible() -> anyhow::Result<()> {
    let _migration_guard = MIGRATION_TEST_LOCK.lock().await;
    let database_url = std::env::var("GRASS_TEST_DATABASE_URL")
        .expect("GRASS_TEST_DATABASE_URL must be set to run this ignored migration test");
    let test_db = PostgresMigrationDatabase::start(&database_url).await?;

    let verification = async {
        Migrator::up(&test_db.db, Some(31)).await?;
        assert_migration_tracking(&test_db.db, 31).await?;
        assert_avatar_schema(&test_db.db).await?;
        assert_screenshot_schema(&test_db.db).await?;
        assert_object_storage_schema(&test_db.db).await?;

        Migrator::down(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 30).await?;
        assert_regional_ingress_lifecycle_absent(&test_db.db).await?;
        assert_avatar_schema(&test_db.db).await?;
        assert_screenshot_schema(&test_db.db).await?;
        assert_object_storage_schema(&test_db.db).await?;

        Migrator::down(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 29).await?;
        assert_regional_ingress_schema_absent(&test_db.db).await?;
        assert_avatar_schema(&test_db.db).await?;
        assert_screenshot_schema(&test_db.db).await?;
        assert_object_storage_schema(&test_db.db).await?;

        Migrator::down(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 28).await?;
        assert_avatar_schema(&test_db.db).await?;
        assert_screenshot_schema(&test_db.db).await?;
        assert_object_storage_schema(&test_db.db).await?;

        Migrator::down(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 27).await?;
        assert_avatar_schema(&test_db.db).await?;
        assert_screenshot_schema(&test_db.db).await?;
        assert_object_storage_schema_absent(&test_db.db).await?;

        Migrator::up(&test_db.db, Some(4)).await?;
        assert_migration_tracking(&test_db.db, 31).await?;
        assert_avatar_schema(&test_db.db).await?;
        assert_screenshot_schema(&test_db.db).await?;
        assert_object_storage_schema(&test_db.db).await
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

async fn assert_avatar_schema(db: &DatabaseConnection) -> anyhow::Result<()> {
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT table_name, udt_name, is_nullable, column_default
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND column_name = 'avatar_version'
  AND table_name IN ('teams', 'users')
ORDER BY table_name
"#,
        ))
        .await?
        .into_iter()
        .map(|row| {
            Ok((
                row.try_get::<String>("", "table_name")?,
                row.try_get::<String>("", "udt_name")?,
                row.try_get::<String>("", "is_nullable")?,
                row.try_get::<Option<String>>("", "column_default")?,
            ))
        })
        .collect::<Result<Vec<_>, sea_orm::DbErr>>()?;
    ensure!(
        rows == vec![
            (
                "teams".to_owned(),
                "uuid".to_owned(),
                "YES".to_owned(),
                None,
            ),
            (
                "users".to_owned(),
                "uuid".to_owned(),
                "YES".to_owned(),
                None,
            ),
        ],
        "unexpected avatar columns: {rows:#?}"
    );
    Ok(())
}

async fn assert_screenshot_schema(db: &DatabaseConnection) -> anyhow::Result<()> {
    let enum_rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT t.typname, string_agg(e.enumlabel, ',' ORDER BY e.enumsortorder) AS labels
FROM pg_type t
JOIN pg_enum e ON e.enumtypid = t.oid
JOIN pg_namespace n ON n.oid = t.typnamespace
WHERE n.nspname = current_schema()
  AND t.typname IN ('deployment_artifact_kind', 'deployment_screenshot_status')
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
        enum_rows
            == vec![
                (
                    "deployment_artifact_kind".to_owned(),
                    "grass_output,build_log,static_site,screenshot".to_owned(),
                ),
                (
                    "deployment_screenshot_status".to_owned(),
                    "pending,running,succeeded,failed".to_owned(),
                ),
            ],
        "unexpected screenshot enum values: {enum_rows:#?}"
    );

    let columns = query_column_shapes(
        db,
        r#"
SELECT column_name, udt_name, is_nullable, column_default
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND table_name = 'deployment_screenshot_jobs'
ORDER BY ordinal_position
"#,
    )
    .await?;
    ensure!(
        columns.len() == 8
            && columns[0] == column("deployment_id", "uuid", "NO", None)
            && columns[1].name == "status"
            && columns[1].udt_name == "deployment_screenshot_status"
            && columns[1].nullable == "NO"
            && columns[1]
                .default
                .as_deref()
                .is_some_and(|value| value.contains("'pending'"))
            && columns[2].name == "attempt_count"
            && columns[2].udt_name == "int4"
            && columns[2].nullable == "NO"
            && columns[2].default.as_deref() == Some("0")
            && columns[3] == column("next_attempt_at", "timestamptz", "NO", None)
            && columns[4] == column("last_error", "text", "YES", None)
            && columns[5] == column("artifact_id", "uuid", "YES", None)
            && columns[6] == column("created_at", "timestamptz", "NO", None)
            && columns[7] == column("updated_at", "timestamptz", "NO", None),
        "unexpected screenshot job columns: {columns:#?}"
    );

    let constraints = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT conname, pg_get_constraintdef(oid) AS definition
FROM pg_constraint
WHERE conrelid = 'deployment_screenshot_jobs'::regclass
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
        constraints
            .get("deployment_screenshot_jobs_deployment_id_fkey")
            .is_some_and(|value| value.contains("ON DELETE CASCADE")),
        "screenshot deployment foreign key must cascade"
    );
    ensure!(
        constraints
            .get("deployment_screenshot_jobs_artifact_id_fkey")
            .is_some_and(|value| value.contains("ON DELETE CASCADE")),
        "screenshot artifact foreign key must cascade"
    );
    ensure!(
        constraints
            .get("ck_deployment_screenshot_attempt_count")
            .is_some_and(|value| value.contains("attempt_count <= 4")),
        "screenshot attempt constraint is missing"
    );
    ensure!(
        constraints
            .get("ck_deployment_screenshot_artifact")
            .is_some_and(|value| value.contains("artifact_id IS NOT NULL")),
        "screenshot artifact state constraint is missing"
    );

    let index = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT indexdef
FROM pg_indexes
WHERE schemaname = current_schema()
  AND indexname = 'ix_deployment_screenshot_jobs_due'
"#,
        ))
        .await?
        .context("screenshot due-job index is missing")?;
    let index = index.try_get::<String>("", "indexdef")?;
    ensure!(
        index.contains("next_attempt_at, deployment_id") && index.contains("status = 'pending'"),
        "unexpected screenshot due-job index: {index}"
    );
    Ok(())
}

async fn assert_object_storage_schema(db: &DatabaseConnection) -> anyhow::Result<()> {
    let table_count = object_count(
        db,
        r#"
SELECT count(*)::bigint AS count
FROM information_schema.tables
WHERE table_schema = current_schema()
  AND table_name IN ('storage_migration_jobs', 'storage_migration_objects')
"#,
    )
    .await?;
    ensure!(
        table_count == 2,
        "object storage migration tables are incomplete"
    );

    let enum_count = object_count(
        db,
        r#"
SELECT count(*)::bigint AS count
FROM pg_type t
JOIN pg_namespace n ON n.oid = t.typnamespace
WHERE n.nspname = current_schema()
  AND t.typname IN ('storage_migration_status', 'storage_migration_object_status')
"#,
    )
    .await?;
    ensure!(
        enum_count == 2,
        "object storage migration enums are incomplete"
    );

    let index_count = object_count(
        db,
        r#"
SELECT count(*)::bigint AS count
FROM pg_indexes
WHERE schemaname = current_schema()
  AND indexname IN ('ux_storage_migration_jobs_active', 'ix_storage_migration_objects_due')
"#,
    )
    .await?;
    ensure!(
        index_count == 2,
        "object storage migration indexes are incomplete"
    );
    Ok(())
}

async fn assert_object_storage_schema_absent(db: &DatabaseConnection) -> anyhow::Result<()> {
    ensure!(
        object_count(
            db,
            r#"
SELECT count(*)::bigint AS count
FROM information_schema.tables
WHERE table_schema = current_schema()
  AND table_name IN ('storage_migration_jobs', 'storage_migration_objects')
"#,
        )
        .await?
            == 0
    );
    ensure!(
        object_count(
            db,
            r#"
SELECT count(*)::bigint AS count
FROM pg_type t
JOIN pg_namespace n ON n.oid = t.typnamespace
WHERE n.nspname = current_schema()
  AND t.typname IN ('storage_migration_status', 'storage_migration_object_status')
"#,
        )
        .await?
            == 0
    );
    ensure!(
        object_count(
            db,
            r#"
SELECT count(*)::bigint AS count
FROM pg_indexes
WHERE schemaname = current_schema()
  AND indexname IN ('ux_storage_migration_jobs_active', 'ix_storage_migration_objects_due')
"#,
        )
        .await?
            == 0
    );
    Ok(())
}
