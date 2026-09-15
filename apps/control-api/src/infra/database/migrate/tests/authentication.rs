use std::collections::BTreeMap;

use anyhow::{Context, ensure};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;
use uuid::Uuid;

use super::super::{MIGRATION_TEST_LOCK, Migrator};
use super::support::{
    PostgresMigrationDatabase, assert_migration_tracking, column, object_count, query_column_shapes,
};

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL"]
async fn postgres_authentication_schema_matches_domain_and_is_reversible() -> anyhow::Result<()> {
    let _migration_guard = MIGRATION_TEST_LOCK.lock().await;
    let database_url = std::env::var("GRASS_TEST_DATABASE_URL")
        .expect("GRASS_TEST_DATABASE_URL must be set to run this ignored migration test");
    let test_db = PostgresMigrationDatabase::start(&database_url).await?;

    let verification = async {
        Migrator::up(&test_db.db, Some(21)).await?;
        assert_migration_tracking(&test_db.db, 21).await?;
        let user_id = seed_authentication_fixture(&test_db.db).await?;

        Migrator::up(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 22).await?;
        assert_authentication_schema(&test_db.db, user_id).await?;

        Migrator::down(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 21).await?;
        assert_authentication_schema_absent(&test_db.db).await?;

        Migrator::up(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 22).await?;
        assert_authentication_schema(&test_db.db, user_id).await
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

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL"]
async fn postgres_mfa_policy_schema_migrates_legacy_scope_and_is_reversible() -> anyhow::Result<()>
{
    let _migration_guard = MIGRATION_TEST_LOCK.lock().await;
    let database_url = std::env::var("GRASS_TEST_DATABASE_URL")
        .expect("GRASS_TEST_DATABASE_URL must be set to run this ignored migration test");
    let test_db = PostgresMigrationDatabase::start(&database_url).await?;

    let verification = async {
        Migrator::up(&test_db.db, Some(22)).await?;
        assert_migration_tracking(&test_db.db, 22).await?;
        let user_id = seed_legacy_mfa_policy_fixture(&test_db.db).await?;

        Migrator::up(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 23).await?;
        assert_mfa_policy_schema(&test_db.db, user_id).await?;

        Migrator::down(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 22).await?;
        assert_mfa_policy_schema_absent(&test_db.db, user_id).await?;

        Migrator::up(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 23).await?;
        assert_mfa_policy_schema(&test_db.db, user_id).await
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

async fn seed_legacy_mfa_policy_fixture(db: &DatabaseConnection) -> anyhow::Result<Uuid> {
    let user_id = Uuid::now_v7();
    let setting_id = Uuid::now_v7();
    db.execute_unprepared(&format!(
        r#"
INSERT INTO users (id, email, display_name, email_verified_at)
VALUES ('{user_id}'::uuid, 'mfa-policy-migration@example.invalid', 'MFA Policy Migration', NOW());

INSERT INTO system_settings (id, key, value_kind, value, is_secret)
VALUES (
'{setting_id}'::uuid,
'auth.mfa_policy',
'json',
jsonb_build_object(
    'allowed_factors', jsonb_build_array('totp', 'email'),
    'enforcement', 'selected_users',
    'selected_user_ids', jsonb_build_array('{user_id}'::text)
),
false
);
"#
    ))
    .await?;
    Ok(user_id)
}

async fn assert_mfa_policy_schema(
    db: &DatabaseConnection,
    selected_user_id: Uuid,
) -> anyhow::Result<()> {
    let columns = query_column_shapes(
        db,
        r#"
SELECT column_name, udt_name, is_nullable, column_default
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND table_name = 'user_mfa_policies'
ORDER BY ordinal_position
"#,
    )
    .await?;
    ensure!(
        columns
            == vec![
                column("user_id", "uuid", "NO", None),
                column("inherit_platform", "bool", "NO", Some("true")),
                column("minimum_factors", "int2", "NO", Some("0")),
                column("required_factors", "jsonb", "NO", Some("'[]'::jsonb")),
                column("created_at", "timestamptz", "NO", None),
                column("updated_at", "timestamptz", "NO", None),
            ],
        "unexpected user MFA policy columns: {columns:#?}"
    );

    let constraints = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT conname, pg_get_constraintdef(oid) AS definition
FROM pg_constraint
WHERE conrelid = 'user_mfa_policies'::regclass
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
        constraints.len() == 4,
        "missing user MFA policy constraints"
    );
    ensure!(constraints["ck_user_mfa_policies_minimum_factors"].contains("minimum_factors <= 2"));
    ensure!(
        constraints["ck_user_mfa_policies_required_factors"]
            .contains("jsonb_typeof(required_factors)")
            && constraints["ck_user_mfa_policies_required_factors"].contains("'array'")
    );
    ensure!(
        constraints
            .values()
            .any(|definition| definition.contains("FOREIGN KEY (user_id)")
                && definition.contains("REFERENCES users(id) ON DELETE CASCADE"))
    );

    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT
setting.value AS platform_policy,
policy.inherit_platform,
policy.minimum_factors,
policy.required_factors
FROM system_settings AS setting
JOIN user_mfa_policies AS policy ON policy.user_id = $1
WHERE setting.key = 'auth.mfa_policy'
"#,
            [selected_user_id.into()],
        ))
        .await?
        .context("migrated MFA policy row is missing")?;
    let platform_policy = row.try_get::<serde_json::Value>("", "platform_policy")?;
    ensure!(platform_policy["enforcement"] == "none");
    ensure!(platform_policy["minimum_factors"] == 0);
    ensure!(platform_policy["required_factors"] == serde_json::json!([]));
    ensure!(platform_policy.get("selected_user_ids").is_none());
    ensure!(!row.try_get::<bool>("", "inherit_platform")?);
    ensure!(row.try_get::<i16>("", "minimum_factors")? == 1);
    ensure!(row.try_get::<serde_json::Value>("", "required_factors")? == serde_json::json!([]));
    Ok(())
}

async fn assert_mfa_policy_schema_absent(
    db: &DatabaseConnection,
    selected_user_id: Uuid,
) -> anyhow::Result<()> {
    ensure!(
        object_count(
            db,
            "SELECT count(*)::bigint AS count FROM information_schema.tables WHERE \
             table_schema = current_schema() AND table_name = 'user_mfa_policies'",
        )
        .await?
            == 0
    );
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT value FROM system_settings WHERE key = 'auth.mfa_policy'",
        ))
        .await?
        .context("legacy MFA policy row is missing after down migration")?;
    let policy = row.try_get::<serde_json::Value>("", "value")?;
    ensure!(policy["enforcement"] == "selected_users");
    ensure!(policy["selected_user_ids"] == serde_json::json!([selected_user_id]));
    ensure!(policy.get("minimum_factors").is_none());
    ensure!(policy.get("required_factors").is_none());
    Ok(())
}

async fn seed_authentication_fixture(db: &DatabaseConnection) -> anyhow::Result<Uuid> {
    let user_id = Uuid::now_v7();
    let credential_id = Uuid::now_v7();
    db.execute_unprepared(&format!(
        r#"
INSERT INTO users (id, email, display_name)
VALUES ('{user_id}'::uuid, 'authentication-migration@example.invalid', 'Authentication Migration');

INSERT INTO user_password_credentials (id, user_id, password_hash)
VALUES ('{credential_id}'::uuid, '{user_id}'::uuid, 'migration-password-hash');
"#
    ))
    .await?;
    Ok(user_id)
}

async fn assert_authentication_schema(
    db: &DatabaseConnection,
    seeded_user_id: Uuid,
) -> anyhow::Result<()> {
    let columns = query_column_shapes(
        db,
        r#"
SELECT column_name, udt_name, is_nullable, column_default
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND (
(table_name = 'users' AND column_name = 'email_verified_at') OR
(table_name = 'user_auth_tokens' AND column_name = 'used_at') OR
(table_name = 'user_mfa_factors' AND column_name IN ('verified_at', 'last_used_at'))
  )
ORDER BY table_name, ordinal_position
"#,
    )
    .await?;
    ensure!(
        columns.len() == 4,
        "missing authentication lifecycle columns"
    );
    for column in columns {
        ensure!(
            column.udt_name == "timestamptz",
            "unexpected column type: {column:?}"
        );
        ensure!(
            column.nullable == "YES",
            "lifecycle column is not nullable: {column:?}"
        );
        ensure!(
            column.default.is_none(),
            "lifecycle column has a default: {column:?}"
        );
    }

    let enum_rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT t.typname, string_agg(e.enumlabel, ',' ORDER BY e.enumsortorder) AS labels
FROM pg_type t
JOIN pg_enum e ON e.enumtypid = t.oid
JOIN pg_namespace n ON n.oid = t.typnamespace
WHERE n.nspname = current_schema()
  AND t.typname IN ('identity_provider_kind', 'auth_token_kind', 'mfa_factor_kind')
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
        .collect::<Result<BTreeMap<_, _>, sea_orm::DbErr>>()?;
    ensure!(enum_rows.get("identity_provider_kind") == Some(&"oidc,github".to_owned()));
    ensure!(
        enum_rows.get("auth_token_kind") == Some(&"email_verification,password_reset".to_owned())
    );
    ensure!(enum_rows.get("mfa_factor_kind") == Some(&"totp,email".to_owned()));

    let indexes = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT indexname, indexdef
FROM pg_indexes
WHERE schemaname = current_schema()
  AND indexname IN (
'ix_user_external_identities_user_id',
'ix_user_auth_tokens_live',
'ix_user_mfa_factors_verified',
'ix_user_password_history_recent'
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
    ensure!(indexes.len() == 4, "missing authentication indexes");
    ensure!(indexes["ix_user_auth_tokens_live"].contains("WHERE (used_at IS NULL)"));
    ensure!(indexes["ix_user_mfa_factors_verified"].contains("WHERE (verified_at IS NOT NULL)"));

    let constraints = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT conname, pg_get_constraintdef(oid) AS definition
FROM pg_constraint
WHERE conrelid IN (
'auth_identity_providers'::regclass,
'user_external_identities'::regclass,
'user_auth_tokens'::regclass,
'user_mfa_factors'::regclass,
'user_password_history'::regclass
)
  AND contype = 'f'
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
        constraints.len() == 6,
        "missing authentication foreign keys"
    );
    ensure!(
        constraints
            .values()
            .filter(|definition| definition.contains("ON DELETE CASCADE"))
            .count()
            == 5
    );
    ensure!(
        constraints
            .values()
            .filter(|definition| definition.contains("ON DELETE SET NULL"))
            .count()
            == 1
    );

    let backfill = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            format!(
                r#"
SELECT
u.email_verified_at = u.created_at AS email_backfilled,
h.password_hash
FROM users u
JOIN user_password_history h ON h.user_id = u.id
WHERE u.id = '{seeded_user_id}'::uuid
"#
            ),
        ))
        .await?
        .context("authentication backfill row is missing")?;
    ensure!(backfill.try_get::<bool>("", "email_backfilled")?);
    ensure!(backfill.try_get::<String>("", "password_hash")? == "migration-password-hash");
    Ok(())
}

async fn assert_authentication_schema_absent(db: &DatabaseConnection) -> anyhow::Result<()> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT
to_regclass('auth_identity_providers') IS NULL AND
to_regclass('user_external_identities') IS NULL AND
to_regclass('user_auth_tokens') IS NULL AND
to_regclass('user_mfa_factors') IS NULL AND
to_regclass('user_password_history') IS NULL AS tables_absent,
NOT EXISTS (
    SELECT 1 FROM information_schema.columns
    WHERE table_schema = current_schema()
      AND table_name = 'users'
      AND column_name = 'email_verified_at'
) AS column_absent,
NOT EXISTS (
    SELECT 1 FROM pg_type t
    JOIN pg_namespace n ON n.oid = t.typnamespace
    WHERE n.nspname = current_schema()
      AND t.typname IN ('identity_provider_kind', 'auth_token_kind', 'mfa_factor_kind')
) AS types_absent
"#,
        ))
        .await?
        .context("authentication absence query returned no row")?;
    ensure!(row.try_get::<bool>("", "tables_absent")?);
    ensure!(row.try_get::<bool>("", "column_absent")?);
    ensure!(row.try_get::<bool>("", "types_absent")?);
    Ok(())
}
