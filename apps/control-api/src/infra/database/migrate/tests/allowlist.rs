use super::super::{MIGRATION_TEST_LOCK, Migrator};
use super::support::{
    PostgresMigrationDatabase, assert_migration_tracking, column, query_column_shapes,
};
use anyhow::{Context, ensure};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;
use std::collections::BTreeMap;

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL"]
async fn postgres_registration_allowlist_schema_matches_domain_and_is_reversible()
-> anyhow::Result<()> {
    let _migration_guard = MIGRATION_TEST_LOCK.lock().await;
    let database_url = std::env::var("GRASS_TEST_DATABASE_URL")
        .expect("GRASS_TEST_DATABASE_URL must be set to run this ignored migration test");
    let test_db = PostgresMigrationDatabase::start(&database_url).await?;

    let verification = async {
        Migrator::up(&test_db.db, Some(25)).await?;
        assert_migration_tracking(&test_db.db, 25).await?;
        assert_registration_allowlist_schema(&test_db.db).await?;

        Migrator::down(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 24).await?;
        assert_registration_allowlist_schema_absent(&test_db.db).await?;

        Migrator::up(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 25).await?;
        assert_registration_allowlist_schema(&test_db.db).await
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

async fn assert_registration_allowlist_schema(db: &DatabaseConnection) -> anyhow::Result<()> {
    let columns = query_column_shapes(
        db,
        r#"
SELECT column_name, udt_name, is_nullable, column_default
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND table_name = 'registration_email_allowlist'
ORDER BY ordinal_position
"#,
    )
    .await?;
    ensure!(
        columns
            == vec![
                column("id", "uuid", "NO", None),
                column("email", "text", "NO", None),
                column("created_by_user_id", "uuid", "YES", None),
                column("created_at", "timestamptz", "NO", None),
            ],
        "unexpected registration allowlist columns: {columns:#?}"
    );

    let definitions = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT conname, pg_get_constraintdef(oid) AS definition
FROM pg_constraint
WHERE conrelid = 'registration_email_allowlist'::regclass
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
        definitions
            .get("registration_email_allowlist_email_key")
            .is_some_and(|definition| definition.starts_with("UNIQUE (email)")),
        "registration allowlist email unique constraint is missing"
    );
    ensure!(
        definitions
            .get("registration_email_allowlist_created_by_user_id_fkey")
            .is_some_and(|definition| definition.contains("ON DELETE SET NULL")),
        "registration allowlist creator foreign key is missing"
    );
    ensure!(
        definitions
            .get("ck_registration_email_allowlist_email")
            .is_some_and(|definition| definition.contains("email = lower(email)")),
        "registration allowlist normalization check is missing"
    );

    let index = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT indexdef
FROM pg_indexes
WHERE schemaname = current_schema()
  AND indexname = 'ix_registration_email_allowlist_created'
"#,
        ))
        .await?
        .context("registration allowlist creation index is missing")?;
    let index_definition = index.try_get::<String>("", "indexdef")?;
    ensure!(
        index_definition.contains("created_at DESC, id DESC"),
        "unexpected registration allowlist index: {index_definition}"
    );
    Ok(())
}

async fn assert_registration_allowlist_schema_absent(
    db: &DatabaseConnection,
) -> anyhow::Result<()> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT to_regclass('registration_email_allowlist') IS NULL AS absent",
        ))
        .await?
        .context("registration allowlist absence query returned no row")?;
    ensure!(row.try_get::<bool>("", "absent")?);
    Ok(())
}
