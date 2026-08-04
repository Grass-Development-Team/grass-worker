use sea_orm::DatabaseConnection;
use sea_orm_migration::{MigratorTrait, prelude::*};

use super::migration;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(migration::m20260515_000001_bootstrap::Migration),
            Box::new(migration::m20260525_000002_lifecycle::Migration),
            Box::new(migration::m20260710_000003_team_invitation_tokens::Migration),
            Box::new(migration::m20260714_000004_m0_m2_remediation::Migration),
            Box::new(migration::m20260720_000005_platform_admin_role::Migration),
            Box::new(migration::m20260726_000006_project_host_columns::Migration),
            Box::new(migration::m20260726_000007_deployment_stage::Migration),
            Box::new(migration::m20260726_000008_audit_team_scope::Migration),
            Box::new(migration::m20260727_000009_git_source_access::Migration),
            Box::new(migration::m20260727_000010_node_scheduling::Migration),
            Box::new(migration::m20260728_000011_delivery_rollout::Migration),
            Box::new(migration::m20260729_000012_audit_foundation::Migration),
            Box::new(migration::m20260729_000013_team_group_review_policy::Migration),
            Box::new(migration::m20260729_000014_node_config_sync::Migration),
            Box::new(migration::m20260729_000015_node_deletion_queue::Migration),
            Box::new(migration::m20260730_000016_domain_review_policy::Migration),
            Box::new(migration::m20260731_000017_project_notifications::Migration),
            Box::new(migration::m20260801_000018_artifact_retention::Migration),
            Box::new(migration::m20260801_000019_ssr_process_leases::Migration),
            Box::new(migration::m20260803_000020_notification_content::Migration),
            Box::new(migration::m20260803_000021_announcements::Migration),
            Box::new(migration::m20260804_000022_authentication::Migration),
        ]
    }
}

pub async fn run(database: &DatabaseConnection) -> anyhow::Result<()> {
    Migrator::up(database, None)
        .await
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("failed to run database migrations: {error}"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use anyhow::{Context, ensure};
    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};
    use uuid::Uuid;

    use super::*;

    static MIGRATION_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[derive(Debug, Eq, PartialEq)]
    struct ColumnShape {
        name: String,
        udt_name: String,
        nullable: String,
        default: Option<String>,
    }

    struct PostgresMigrationDatabase {
        db: DatabaseConnection,
        admin: DatabaseConnection,
        schema: String,
    }

    impl PostgresMigrationDatabase {
        async fn start(database_url: &str) -> anyhow::Result<Self> {
            let admin = Database::connect(database_url).await?;
            let schema = format!("gw_audit_migration_{}", Uuid::now_v7().simple());
            admin
                .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
                .await?;

            let mut scoped_url = url::Url::parse(database_url)?;
            scoped_url
                .query_pairs_mut()
                .append_pair("options", &format!("-csearch_path={schema}"));
            let db = match Database::connect(scoped_url.as_str()).await {
                Ok(db) => db,
                Err(error) => {
                    admin
                        .execute_unprepared(&format!("DROP SCHEMA {schema} CASCADE"))
                        .await?;
                    return Err(error.into());
                }
            };

            Ok(Self { db, admin, schema })
        }

        async fn cleanup(self) -> anyhow::Result<()> {
            self.db.close().await?;
            self.admin
                .execute_unprepared(&format!("DROP SCHEMA {} CASCADE", self.schema))
                .await?;
            self.admin.close().await?;
            Ok(())
        }
    }

    #[test]
    fn registers_audit_foundation_migration() {
        let migrations = Migrator::migrations();

        assert_eq!(migrations.len(), 22);
        assert_eq!(
            migrations.get(11).expect("twelfth migration").name(),
            "m20260729_000012_audit_foundation"
        );

        let sql = migration::m20260729_000012_audit_foundation::UP_SQL;
        assert!(sql.contains("CREATE TYPE audit_actor_type"));
        assert!(sql.contains("CREATE TYPE audit_event_visibility"));
        assert!(sql.contains("ADD COLUMN request_id UUID NULL"));
        assert!(sql.contains("ADD COLUMN changes JSONB NOT NULL DEFAULT '{}'"));
        assert!(
            sql.contains("ADD COLUMN pending_release_audit_visibility audit_event_visibility NULL")
        );
        assert!(sql.contains("SET pending_release_audit_visibility = 'platform'"));
        assert!(sql.contains("ck_deployments_pending_release_audit_visibility"));
        assert!(sql.contains("actor_user_id IS NULL OR actor_type = 'user'"));
        assert!(sql.contains("actor_node_id IS NULL OR actor_type = 'node'"));
        assert!(sql.contains("actor_type NOT IN ('anonymous', 'system')"));
        assert!(sql.contains("WHEN actor_user_id IS NOT NULL THEN 'user'"));
        assert!(sql.contains("COALESCE(metadata ->> 'platform_admin', 'false') <> 'true'"));
        assert!(sql.contains("COALESCE(metadata ->> 'completed_after_sync', 'false') <> 'true'"));
        assert!(sql.contains("'team.quota_plan_overridden'"));
    }

    #[test]
    fn registers_team_group_review_policy_migration() {
        let migrations = Migrator::migrations();

        assert_eq!(migrations.len(), 22);
        assert_eq!(
            migrations.get(12).expect("thirteenth migration").name(),
            "m20260729_000013_team_group_review_policy"
        );
    }

    #[test]
    fn registers_node_config_sync_migration() {
        let migrations = Migrator::migrations();

        assert_eq!(migrations.len(), 22);
        assert_eq!(
            migrations.get(13).expect("fourteenth migration").name(),
            "m20260729_000014_node_config_sync"
        );
    }

    #[test]
    fn registers_node_deletion_queue_migration() {
        let migrations = Migrator::migrations();

        assert_eq!(migrations.len(), 22);
        assert_eq!(
            migrations.get(14).expect("fifteenth migration").name(),
            "m20260729_000015_node_deletion_queue"
        );
    }

    #[test]
    fn registers_domain_review_policy_after_node_deletion_queue() {
        let migrations = Migrator::migrations();

        assert_eq!(migrations.len(), 22);
        assert_eq!(
            migrations.get(14).expect("fifteenth migration").name(),
            "m20260729_000015_node_deletion_queue"
        );
        assert_eq!(
            migrations.get(15).expect("sixteenth migration").name(),
            "m20260730_000016_domain_review_policy"
        );
    }

    #[test]
    fn registers_project_notifications_after_domain_review_policy() {
        let migrations = Migrator::migrations();

        assert_eq!(migrations.len(), 22);
        assert_eq!(
            migrations.get(15).expect("sixteenth migration").name(),
            "m20260730_000016_domain_review_policy"
        );
        assert_eq!(
            migrations.get(16).expect("seventeenth migration").name(),
            "m20260731_000017_project_notifications"
        );
        assert_eq!(
            migrations.get(17).expect("eighteenth migration").name(),
            "m20260801_000018_artifact_retention"
        );
        assert_eq!(
            migrations.last().expect("last migration").name(),
            "m20260804_000022_authentication"
        );
    }

    #[tokio::test]
    #[ignore = "requires GRASS_TEST_DATABASE_URL"]
    async fn postgres_notification_and_announcement_schema_matches_the_domain_model_and_is_reversible()
    -> anyhow::Result<()> {
        let _migration_guard = MIGRATION_TEST_LOCK.lock().await;
        let database_url = std::env::var("GRASS_TEST_DATABASE_URL")
            .expect("GRASS_TEST_DATABASE_URL must be set to run this ignored migration test");
        let test_db = PostgresMigrationDatabase::start(&database_url).await?;

        let verification = async {
            Migrator::up(&test_db.db, Some(19)).await?;
            assert_migration_tracking(&test_db.db, 19, 2).await?;

            Migrator::up(&test_db.db, Some(1)).await?;
            assert_migration_tracking(&test_db.db, 20, 1).await?;
            assert_notification_content_schema(&test_db.db).await?;

            Migrator::up(&test_db.db, Some(1)).await?;
            assert_migration_tracking(&test_db.db, 21, 0).await?;
            assert_announcement_schema(&test_db.db).await?;

            Migrator::down(&test_db.db, Some(1)).await?;
            assert_migration_tracking(&test_db.db, 20, 1).await?;
            assert_announcement_schema_absent(&test_db.db).await?;
            assert_notification_content_schema(&test_db.db).await?;

            Migrator::up(&test_db.db, Some(1)).await?;
            assert_migration_tracking(&test_db.db, 21, 0).await?;
            assert_announcement_schema(&test_db.db).await
        }
        .await;
        let cleanup = test_db.cleanup().await;

        match (verification, cleanup) {
            (Err(verification_error), Err(cleanup_error)) => Err(verification_error.context(
                format!("disposable schema cleanup also failed: {cleanup_error:#}"),
            )),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    #[tokio::test]
    #[ignore = "requires GRASS_TEST_DATABASE_URL"]
    async fn postgres_audit_foundation_migration_upgrades_v11_and_is_reversible()
    -> anyhow::Result<()> {
        let _migration_guard = MIGRATION_TEST_LOCK.lock().await;
        let database_url = std::env::var("GRASS_TEST_DATABASE_URL")
            .expect("GRASS_TEST_DATABASE_URL must be set to run this ignored migration test");
        let test_db = PostgresMigrationDatabase::start(&database_url).await?;

        let verification = verify_audit_foundation_migration(&test_db.db).await;
        let cleanup = test_db.cleanup().await;

        match (verification, cleanup) {
            (Err(verification_error), Err(cleanup_error)) => Err(verification_error.context(
                format!("disposable schema cleanup also failed: {cleanup_error:#}"),
            )),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    #[tokio::test]
    #[ignore = "requires GRASS_TEST_DATABASE_URL"]
    async fn postgres_node_deletion_queue_schema_matches_domain_and_is_reversible()
    -> anyhow::Result<()> {
        let _migration_guard = MIGRATION_TEST_LOCK.lock().await;
        let database_url = std::env::var("GRASS_TEST_DATABASE_URL")
            .expect("GRASS_TEST_DATABASE_URL must be set to run this ignored migration test");
        let test_db = PostgresMigrationDatabase::start(&database_url).await?;

        let verification = async {
            Migrator::up(&test_db.db, Some(15)).await?;
            assert_migration_tracking(&test_db.db, 15, 2).await?;
            assert_node_deletion_schema(&test_db.db).await?;

            Migrator::down(&test_db.db, Some(1)).await?;
            assert_migration_tracking(&test_db.db, 14, 3).await?;
            assert_node_deletion_schema_absent(&test_db.db).await?;

            Migrator::up(&test_db.db, Some(1)).await?;
            assert_migration_tracking(&test_db.db, 15, 2).await?;
            assert_node_deletion_schema(&test_db.db).await
        }
        .await;
        let cleanup = test_db.cleanup().await;

        match (verification, cleanup) {
            (Err(verification_error), Err(cleanup_error)) => Err(verification_error.context(
                format!("disposable schema cleanup also failed: {cleanup_error:#}"),
            )),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    #[tokio::test]
    #[ignore = "requires GRASS_TEST_DATABASE_URL"]
    async fn postgres_project_notification_schema_backfills_and_is_reversible() -> anyhow::Result<()>
    {
        let _migration_guard = MIGRATION_TEST_LOCK.lock().await;
        let database_url = std::env::var("GRASS_TEST_DATABASE_URL")
            .expect("GRASS_TEST_DATABASE_URL must be set to run this ignored migration test");
        let test_db = PostgresMigrationDatabase::start(&database_url).await?;

        let verification = async {
            Migrator::up(&test_db.db, Some(16)).await?;
            assert_migration_tracking(&test_db.db, 16, 1).await?;
            let (user_id, project_id) = seed_project_notification_fixture(&test_db.db).await?;

            Migrator::up(&test_db.db, Some(1)).await?;
            assert_migration_tracking(&test_db.db, 17, 0).await?;
            assert_project_notification_schema(&test_db.db, user_id, project_id).await?;

            Migrator::down(&test_db.db, Some(1)).await?;
            assert_migration_tracking(&test_db.db, 16, 1).await?;
            assert_project_notification_schema_absent(&test_db.db).await?;

            Migrator::up(&test_db.db, Some(1)).await?;
            assert_migration_tracking(&test_db.db, 17, 0).await?;
            assert_project_notification_schema(&test_db.db, user_id, project_id).await
        }
        .await;
        let cleanup = test_db.cleanup().await;

        match (verification, cleanup) {
            (Err(verification_error), Err(cleanup_error)) => Err(verification_error.context(
                format!("disposable schema cleanup also failed: {cleanup_error:#}"),
            )),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    #[tokio::test]
    #[ignore = "requires GRASS_TEST_DATABASE_URL"]
    async fn postgres_authentication_schema_matches_domain_and_is_reversible() -> anyhow::Result<()>
    {
        let _migration_guard = MIGRATION_TEST_LOCK.lock().await;
        let database_url = std::env::var("GRASS_TEST_DATABASE_URL")
            .expect("GRASS_TEST_DATABASE_URL must be set to run this ignored migration test");
        let test_db = PostgresMigrationDatabase::start(&database_url).await?;

        let verification = async {
            Migrator::up(&test_db.db, Some(21)).await?;
            assert_migration_tracking(&test_db.db, 21, 1).await?;
            let user_id = seed_authentication_fixture(&test_db.db).await?;

            Migrator::up(&test_db.db, Some(1)).await?;
            assert_migration_tracking(&test_db.db, 22, 0).await?;
            assert_authentication_schema(&test_db.db, user_id).await?;

            Migrator::down(&test_db.db, Some(1)).await?;
            assert_migration_tracking(&test_db.db, 21, 1).await?;
            assert_authentication_schema_absent(&test_db.db).await?;

            Migrator::up(&test_db.db, Some(1)).await?;
            assert_migration_tracking(&test_db.db, 22, 0).await?;
            assert_authentication_schema(&test_db.db, user_id).await
        }
        .await;
        let cleanup = test_db.cleanup().await;

        match (verification, cleanup) {
            (Err(verification_error), Err(cleanup_error)) => Err(verification_error.context(
                format!("disposable schema cleanup also failed: {cleanup_error:#}"),
            )),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
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
WHERE t.typname IN ('identity_provider_kind', 'auth_token_kind', 'mfa_factor_kind')
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
            enum_rows.get("auth_token_kind")
                == Some(&"email_verification,password_reset".to_owned())
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
        ensure!(
            indexes["ix_user_mfa_factors_verified"].contains("WHERE (verified_at IS NOT NULL)")
        );

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
        SELECT 1 FROM pg_type
        WHERE typname IN ('identity_provider_kind', 'auth_token_kind', 'mfa_factor_kind')
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

    async fn seed_project_notification_fixture(
        db: &DatabaseConnection,
    ) -> anyhow::Result<(Uuid, Uuid)> {
        let user_id = Uuid::now_v7();
        let team_id = Uuid::now_v7();
        let project_id = Uuid::now_v7();
        let audit_id = Uuid::now_v7();
        db.execute_unprepared(&format!(
            r#"
INSERT INTO users (id, email, display_name)
VALUES ('{user_id}'::uuid, 'notification-migration@example.invalid', 'Notification Migration');

INSERT INTO teams (id, slug, name, owner_user_id)
VALUES ('{team_id}'::uuid, 'notification-migration', 'Notification Migration', '{user_id}'::uuid);

INSERT INTO projects (id, team_id, slug, name)
VALUES ('{project_id}'::uuid, '{team_id}'::uuid, 'notification-migration', 'Notification Migration');

INSERT INTO audit_events (
    id,
    actor_user_id,
    actor_type,
    visibility,
    action,
    target_type,
    target_id,
    result,
    metadata,
    team_id
)
VALUES (
    '{audit_id}'::uuid,
    '{user_id}'::uuid,
    'user',
    'team',
    'project.created',
    'project',
    '{project_id}'::uuid,
    'success',
    '{{}}'::jsonb,
    '{team_id}'::uuid
);
"#
        ))
        .await?;
        Ok((user_id, project_id))
    }

    async fn assert_project_notification_schema(
        db: &DatabaseConnection,
        expected_creator_id: Uuid,
        project_id: Uuid,
    ) -> anyhow::Result<()> {
        let project_columns = query_column_shapes(
            db,
            r#"
SELECT column_name, udt_name, is_nullable, column_default
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND table_name = 'projects'
  AND column_name = 'created_by_user_id'
"#,
        )
        .await?;
        ensure!(
            project_columns
                == vec![ColumnShape {
                    name: "created_by_user_id".to_owned(),
                    udt_name: "uuid".to_owned(),
                    nullable: "YES".to_owned(),
                    default: None,
                }],
            "unexpected Project creator column: {project_columns:?}"
        );

        let notification_columns = query_column_shapes(
            db,
            r#"
SELECT column_name, udt_name, is_nullable, column_default
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND table_name = 'user_notifications'
ORDER BY ordinal_position
"#,
        )
        .await?;
        ensure!(
            notification_columns.len() == 13,
            "expected 13 notification columns, found {}",
            notification_columns.len()
        );
        let shapes = notification_columns
            .into_iter()
            .map(|column| (column.name.clone(), column))
            .collect::<BTreeMap<_, _>>();
        ensure!(
            shapes.get("recipient_user_id")
                == Some(&ColumnShape {
                    name: "recipient_user_id".to_owned(),
                    udt_name: "uuid".to_owned(),
                    nullable: "NO".to_owned(),
                    default: None,
                })
        );
        ensure!(
            shapes.get("read_at")
                == Some(&ColumnShape {
                    name: "read_at".to_owned(),
                    udt_name: "timestamptz".to_owned(),
                    nullable: "YES".to_owned(),
                    default: None,
                })
        );

        let constraints = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT conname, pg_get_constraintdef(oid) AS definition
FROM pg_constraint
WHERE conname IN (
    'fk_projects_created_by_user_id',
    'fk_user_notifications_recipient_user_id',
    'fk_user_notifications_actor_user_id',
    'fk_user_notifications_team_id',
    'fk_user_notifications_project_id'
)
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
        ensure!(constraints.len() == 5, "missing notification foreign keys");
        ensure!(
            constraints["fk_user_notifications_recipient_user_id"].contains("ON DELETE CASCADE")
        );
        for name in [
            "fk_projects_created_by_user_id",
            "fk_user_notifications_actor_user_id",
            "fk_user_notifications_team_id",
            "fk_user_notifications_project_id",
        ] {
            ensure!(
                constraints[name].contains("ON DELETE SET NULL"),
                "{name} must preserve notification history"
            );
        }

        let indexes = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT indexname, indexdef
FROM pg_indexes
WHERE schemaname = current_schema()
  AND indexname IN (
    'ix_projects_created_by_user_id',
    'ix_user_notifications_recipient_created',
    'ix_user_notifications_recipient_unread'
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
        ensure!(indexes.len() == 3, "missing notification indexes");
        ensure!(indexes["ix_user_notifications_recipient_unread"].contains("read_at IS NULL"));

        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT created_by_user_id FROM projects WHERE id = $1",
                [project_id.into()],
            ))
            .await?
            .context("Project creator backfill query returned no row")?;
        ensure!(
            row.try_get::<Uuid>("", "created_by_user_id")? == expected_creator_id,
            "Project creator was not backfilled from the creation audit"
        );
        Ok(())
    }

    async fn assert_project_notification_schema_absent(
        db: &DatabaseConnection,
    ) -> anyhow::Result<()> {
        let row = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT
  to_regclass('user_notifications') IS NULL AS notifications_absent,
  NOT EXISTS (
    SELECT 1
    FROM information_schema.columns
    WHERE table_schema = current_schema()
      AND table_name = 'projects'
      AND column_name = 'created_by_user_id'
  ) AS creator_absent
"#,
            ))
            .await?
            .context("notification absence query returned no row")?;
        ensure!(row.try_get::<bool>("", "notifications_absent")?);
        ensure!(row.try_get::<bool>("", "creator_absent")?);
        Ok(())
    }

    async fn assert_notification_content_schema(db: &DatabaseConnection) -> anyhow::Result<()> {
        let columns = query_column_shapes(
            db,
            r#"
SELECT column_name, udt_name, is_nullable, column_default
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND table_name = 'user_notifications'
  AND column_name IN ('project_name', 'project_slug', 'title', 'content')
ORDER BY ordinal_position
"#,
        )
        .await?;
        ensure!(
            columns
                == vec![
                    ColumnShape {
                        name: "project_name".to_owned(),
                        udt_name: "text".to_owned(),
                        nullable: "YES".to_owned(),
                        default: None,
                    },
                    ColumnShape {
                        name: "project_slug".to_owned(),
                        udt_name: "text".to_owned(),
                        nullable: "YES".to_owned(),
                        default: None,
                    },
                    ColumnShape {
                        name: "title".to_owned(),
                        udt_name: "text".to_owned(),
                        nullable: "YES".to_owned(),
                        default: None,
                    },
                    ColumnShape {
                        name: "content".to_owned(),
                        udt_name: "text".to_owned(),
                        nullable: "YES".to_owned(),
                        default: None,
                    },
                ],
            "unexpected notification content columns: {columns:?}"
        );

        let row = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT pg_get_constraintdef(oid) AS definition
FROM pg_constraint
WHERE conname = 'ck_user_notifications_announcement_content'
"#,
            ))
            .await?
            .context("announcement content constraint was not created")?;
        let definition = row.try_get::<String>("", "definition")?;
        ensure!(definition.contains("site.announcement"));
        ensure!(definition.contains("team_id IS NULL"));
        ensure!(definition.contains("project_id IS NULL"));
        Ok(())
    }

    async fn assert_announcement_schema(db: &DatabaseConnection) -> anyhow::Result<()> {
        let columns = query_column_shapes(
            db,
            r#"
SELECT column_name, udt_name, is_nullable, column_default
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND table_name = 'announcements'
ORDER BY ordinal_position
"#,
        )
        .await?;
        ensure!(
            columns
                == vec![
                    column("id", "uuid", "NO", None),
                    column("title", "text", "NO", None),
                    column("content", "text", "NO", None),
                    column("auto_popup", "bool", "NO", Some("false")),
                    column("created_by_user_id", "uuid", "YES", None),
                    column("published_at", "timestamptz", "NO", None),
                ],
            "unexpected announcement columns: {columns:#?}"
        );

        let notification_columns = query_column_shapes(
            db,
            r#"
SELECT column_name, udt_name, is_nullable, column_default
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND table_name = 'user_notifications'
  AND column_name = 'announcement_id'
"#,
        )
        .await?;
        ensure!(
            notification_columns == vec![column("announcement_id", "uuid", "YES", None)],
            "unexpected notification announcement column: {notification_columns:#?}"
        );

        let constraints = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT conname, pg_get_constraintdef(oid) AS definition
FROM pg_constraint
WHERE conname IN (
    'ck_announcements_title_length',
    'ck_announcements_content_length',
    'ck_user_notifications_announcement_content'
)
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
        ensure!(constraints.len() == 3, "missing announcement constraints");
        ensure!(constraints["ck_announcements_title_length"].contains("120"));
        ensure!(constraints["ck_announcements_content_length"].contains("10000"));
        ensure!(
            constraints["ck_user_notifications_announcement_content"]
                .contains("announcement_id IS NOT NULL")
        );

        let foreign_keys = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT pg_get_constraintdef(oid) AS definition
FROM pg_constraint
WHERE contype = 'f'
  AND conrelid IN ('announcements'::regclass, 'user_notifications'::regclass)
"#,
            ))
            .await?
            .into_iter()
            .map(|row| row.try_get::<String>("", "definition"))
            .collect::<Result<Vec<_>, sea_orm::DbErr>>()?;
        ensure!(
            foreign_keys
                .iter()
                .any(|definition| definition.contains("REFERENCES announcements")
                    && definition.contains("ON DELETE CASCADE")),
            "notification announcement foreign key is not cascading"
        );
        ensure!(
            foreign_keys
                .iter()
                .any(|definition| definition.contains("REFERENCES users")
                    && definition.contains("ON DELETE SET NULL")),
            "announcement creator foreign key is not nullable"
        );

        let indexes = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT indexname
FROM pg_indexes
WHERE schemaname = current_schema()
  AND indexname = 'ix_announcements_published_at'
"#,
            ))
            .await?;
        ensure!(indexes.len() == 1, "announcement history index is missing");
        Ok(())
    }

    async fn assert_announcement_schema_absent(db: &DatabaseConnection) -> anyhow::Result<()> {
        let row = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT
  to_regclass('announcements') IS NULL AS table_absent,
  NOT EXISTS (
    SELECT 1
    FROM information_schema.columns
    WHERE table_schema = current_schema()
      AND table_name = 'user_notifications'
      AND column_name = 'announcement_id'
  ) AS notification_column_absent,
  EXISTS (
    SELECT 1
    FROM pg_constraint
    WHERE conname = 'ck_user_notifications_announcement_content'
  ) AS legacy_constraint_present
"#,
            ))
            .await?
            .context("announcement absence query returned no row")?;
        ensure!(row.try_get::<bool>("", "table_absent")?);
        ensure!(row.try_get::<bool>("", "notification_column_absent")?);
        ensure!(row.try_get::<bool>("", "legacy_constraint_present")?);
        Ok(())
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
            constraints.values().any(|definition| definition
                .contains("FOREIGN KEY (target_node_id)")
                && definition.contains("REFERENCES nodes(id) ON DELETE RESTRICT")),
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

    async fn verify_audit_foundation_migration(db: &DatabaseConnection) -> anyhow::Result<()> {
        Migrator::up(db, Some(11)).await?;
        assert_migration_tracking(db, 11, 6).await?;

        let user_id = Uuid::now_v7();
        let team_id = Uuid::now_v7();
        let project_id = Uuid::now_v7();
        let deployment_id = Uuid::now_v7();
        seed_v11_audit_fixtures(db, user_id, team_id, project_id, deployment_id).await?;

        Migrator::up(db, Some(1)).await?;
        assert_migration_tracking(db, 12, 5).await?;
        assert_audit_enum_shapes(db).await?;
        assert_audit_column_shapes(db).await?;
        assert_audit_constraints(db).await?;
        assert_audit_indexes(db).await?;
        assert_audit_backfill(db, deployment_id).await?;

        Migrator::down(db, Some(1)).await?;
        assert_migration_tracking(db, 11, 6).await?;
        assert_audit_foundation_objects_absent(db).await?;

        Migrator::up(db, None).await?;
        assert_migration_tracking(db, 17, 0).await?;
        assert_audit_foundation_objects_restored(db).await?;

        Ok(())
    }

    async fn seed_v11_audit_fixtures(
        db: &DatabaseConnection,
        user_id: Uuid,
        team_id: Uuid,
        project_id: Uuid,
        deployment_id: Uuid,
    ) -> anyhow::Result<()> {
        db.execute_unprepared(&format!(
            r#"
INSERT INTO users (id, email, display_name)
VALUES ('{user_id}'::uuid, 'audit-migration@example.invalid', 'Audit Migration');

INSERT INTO teams (id, slug, name, owner_user_id)
VALUES ('{team_id}'::uuid, 'audit-migration', 'Audit Migration', '{user_id}'::uuid);

INSERT INTO projects (id, team_id, slug, name)
VALUES ('{project_id}'::uuid, '{team_id}'::uuid, 'audit-migration', 'Audit Migration');

INSERT INTO audit_events (actor_user_id, action, target_type, metadata, team_id)
VALUES
    ('{user_id}'::uuid, 'project.updated', 'project', '{{}}'::jsonb, '{team_id}'::uuid),
    (NULL, 'project.deleted', 'project', '{{"platform_admin": true}}'::jsonb, '{team_id}'::uuid),
    ('{user_id}'::uuid, 'deployment.release.completed', 'deployment', '{{"completed_after_sync": true}}'::jsonb, '{team_id}'::uuid),
    (NULL, 'team.quota_plan_overridden', 'team', '{{}}'::jsonb, '{team_id}'::uuid);

INSERT INTO deployments (
    id,
    project_id,
    team_id,
    pending_release_reason,
    pending_release_actor_user_id,
    pending_release_requested_at
)
VALUES (
    '{deployment_id}'::uuid,
    '{project_id}'::uuid,
    '{team_id}'::uuid,
    'rollback',
    '{user_id}'::uuid,
    CURRENT_TIMESTAMP
);
"#
        ))
        .await?;

        Ok(())
    }

    async fn assert_migration_tracking(
        db: &DatabaseConnection,
        applied_count: usize,
        pending_count: usize,
    ) -> anyhow::Result<()> {
        let applied = Migrator::get_applied_migrations(db).await?;
        let pending = Migrator::get_pending_migrations(db).await?;

        ensure!(
            applied.len() == applied_count,
            "expected {applied_count} applied migrations, found {}",
            applied.len()
        );
        ensure!(
            pending.len() == pending_count,
            "expected {pending_count} pending migrations, found {}",
            pending.len()
        );
        if applied_count >= 12 {
            ensure!(
                applied.get(11).map(|migration| migration.name())
                    == Some("m20260729_000012_audit_foundation"),
                "audit foundation migration was not the twelfth applied migration"
            );
        }
        if pending_count > 0 {
            let expected = Migrator::migrations()
                .get(applied_count)
                .map(|migration| migration.name().to_owned());
            ensure!(
                pending.first().map(|migration| migration.name()) == expected.as_deref(),
                "migration tracking did not expose the next registered migration first"
            );
        }

        Ok(())
    }

    async fn assert_audit_enum_shapes(db: &DatabaseConnection) -> anyhow::Result<()> {
        let rows = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT t.typname, string_agg(e.enumlabel, ',' ORDER BY e.enumsortorder) AS labels
FROM pg_type t
JOIN pg_enum e ON e.enumtypid = t.oid
JOIN pg_namespace n ON n.oid = t.typnamespace
WHERE n.nspname = current_schema()
  AND t.typname IN ('audit_actor_type', 'audit_event_visibility')
GROUP BY t.typname
ORDER BY t.typname
"#,
            ))
            .await?;
        let enum_shapes = rows
            .into_iter()
            .map(|row| {
                Ok((
                    row.try_get::<String>("", "typname")?,
                    row.try_get::<String>("", "labels")?,
                ))
            })
            .collect::<Result<Vec<_>, sea_orm::DbErr>>()?;

        ensure!(
            enum_shapes
                == vec![
                    (
                        "audit_actor_type".to_owned(),
                        "anonymous,user,system,node".to_owned(),
                    ),
                    (
                        "audit_event_visibility".to_owned(),
                        "platform,team".to_owned(),
                    ),
                ],
            "unexpected audit enum shapes: {enum_shapes:?}"
        );
        Ok(())
    }

    async fn assert_audit_column_shapes(db: &DatabaseConnection) -> anyhow::Result<()> {
        let audit_columns = query_column_shapes(
            db,
            r#"
SELECT column_name, udt_name, is_nullable, column_default
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND table_name = 'audit_events'
  AND column_name IN (
    'actor_type',
    'actor_node_id',
    'visibility',
    'request_id',
    'source_ip',
    'user_agent',
    'http_method',
    'request_path',
    'status_code',
    'duration_ms',
    'changes'
  )
ORDER BY column_name
"#,
        )
        .await?;
        ensure!(
            audit_columns
                == vec![
                    column("actor_node_id", "uuid", "YES", None),
                    column(
                        "actor_type",
                        "audit_actor_type",
                        "NO",
                        Some("'system'::audit_actor_type"),
                    ),
                    column("changes", "jsonb", "NO", Some("'{}'::jsonb")),
                    column("duration_ms", "int8", "YES", None),
                    column("http_method", "text", "YES", None),
                    column("request_id", "uuid", "YES", None),
                    column("request_path", "text", "YES", None),
                    column("source_ip", "text", "YES", None),
                    column("status_code", "int4", "YES", None),
                    column("user_agent", "text", "YES", None),
                    column(
                        "visibility",
                        "audit_event_visibility",
                        "NO",
                        Some("'platform'::audit_event_visibility"),
                    ),
                ],
            "unexpected audit_events column shapes: {audit_columns:#?}"
        );

        let deployment_columns = query_column_shapes(
            db,
            r#"
SELECT column_name, udt_name, is_nullable, column_default
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND table_name = 'deployments'
  AND column_name = 'pending_release_audit_visibility'
"#,
        )
        .await?;
        ensure!(
            deployment_columns
                == vec![column(
                    "pending_release_audit_visibility",
                    "audit_event_visibility",
                    "YES",
                    None,
                )],
            "unexpected deployment provenance column shape: {deployment_columns:#?}"
        );

        Ok(())
    }

    fn column(name: &str, udt_name: &str, nullable: &str, default: Option<&str>) -> ColumnShape {
        ColumnShape {
            name: name.to_owned(),
            udt_name: udt_name.to_owned(),
            nullable: nullable.to_owned(),
            default: default.map(str::to_owned),
        }
    }

    async fn query_column_shapes(
        db: &DatabaseConnection,
        sql: &str,
    ) -> anyhow::Result<Vec<ColumnShape>> {
        db.query_all_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
            .await?
            .into_iter()
            .map(|row| {
                Ok(ColumnShape {
                    name: row.try_get::<String>("", "column_name")?,
                    udt_name: row.try_get::<String>("", "udt_name")?,
                    nullable: row.try_get::<String>("", "is_nullable")?,
                    default: row.try_get::<Option<String>>("", "column_default")?,
                })
            })
            .collect::<Result<Vec<_>, sea_orm::DbErr>>()
            .map_err(Into::into)
    }

    async fn assert_audit_constraints(db: &DatabaseConnection) -> anyhow::Result<()> {
        let rows = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT conname, contype::text AS constraint_type, pg_get_constraintdef(oid) AS definition
FROM pg_constraint
WHERE conrelid = 'audit_events'::regclass
  AND conname IN (
    'fk_audit_events_actor_node_id',
    'ck_audit_events_actor_identity',
    'ck_audit_events_status_code',
    'ck_audit_events_duration_ms'
  )
ORDER BY conname
"#,
            ))
            .await?;
        let constraints = rows
            .into_iter()
            .map(|row| {
                Ok((
                    row.try_get::<String>("", "conname")?,
                    (
                        row.try_get::<String>("", "constraint_type")?,
                        row.try_get::<String>("", "definition")?,
                    ),
                ))
            })
            .collect::<Result<BTreeMap<_, _>, sea_orm::DbErr>>()?;
        ensure!(
            constraints.len() == 4,
            "expected four audit constraints, found {constraints:#?}"
        );

        let (kind, definition) = constraints
            .get("fk_audit_events_actor_node_id")
            .context("missing actor node foreign key")?;
        ensure!(kind == "f", "actor node constraint is not a foreign key");
        ensure!(
            definition.contains("FOREIGN KEY (actor_node_id)")
                && definition.contains("REFERENCES nodes(id)")
                && definition.contains("ON DELETE SET NULL"),
            "unexpected actor node foreign key: {definition}"
        );

        let (kind, definition) = constraints
            .get("ck_audit_events_actor_identity")
            .context("missing actor identity constraint")?;
        ensure!(kind == "c", "actor identity constraint is not a check");
        ensure!(
            definition.contains("actor_user_id IS NULL")
                && definition.contains("actor_type = 'user'")
                && definition.contains("actor_node_id IS NULL")
                && definition.contains("actor_type = 'node'")
                && definition.contains("actor_type <> ALL"),
            "unexpected actor identity check: {definition}"
        );

        let (kind, definition) = constraints
            .get("ck_audit_events_status_code")
            .context("missing status code constraint")?;
        ensure!(kind == "c", "status code constraint is not a check");
        ensure!(
            definition.contains("status_code IS NULL")
                && definition.contains("status_code >= 100")
                && definition.contains("status_code <= 599"),
            "unexpected status code check: {definition}"
        );

        let (kind, definition) = constraints
            .get("ck_audit_events_duration_ms")
            .context("missing duration constraint")?;
        ensure!(kind == "c", "duration constraint is not a check");
        ensure!(
            definition.contains("duration_ms IS NULL") && definition.contains("duration_ms >= 0"),
            "unexpected duration check: {definition}"
        );

        let deployment_constraint = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT contype::text AS constraint_type, pg_get_constraintdef(oid) AS definition
FROM pg_constraint
WHERE conrelid = 'deployments'::regclass
  AND conname = 'ck_deployments_pending_release_audit_visibility'
"#,
            ))
            .await?
            .context("missing pending release audit visibility constraint")?;
        let kind = deployment_constraint.try_get::<String>("", "constraint_type")?;
        let definition = deployment_constraint.try_get::<String>("", "definition")?;
        ensure!(kind == "c", "pending release constraint is not a check");
        ensure!(
            definition.contains("pending_release_reason IS NULL")
                && definition.contains("pending_release_audit_visibility IS NULL"),
            "unexpected pending release audit visibility check: {definition}"
        );

        Ok(())
    }

    async fn assert_audit_indexes(db: &DatabaseConnection) -> anyhow::Result<()> {
        let rows = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT indexname, indexdef
FROM pg_indexes
WHERE schemaname = current_schema()
  AND tablename = 'audit_events'
  AND indexname IN (
    'ux_audit_events_request_id',
    'ix_audit_events_visibility_created_at',
    'ix_audit_events_actor_created_at',
    'ix_audit_events_actor_node_created_at',
    'ix_audit_events_created_at'
  )
ORDER BY indexname
"#,
            ))
            .await?;
        let indexes = rows
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
            "expected five audit indexes, found {indexes:#?}"
        );

        let request = indexes
            .get("ux_audit_events_request_id")
            .context("missing request id index")?;
        ensure!(
            request.contains("CREATE UNIQUE INDEX")
                && request.contains("(request_id)")
                && request.contains("request_id IS NOT NULL"),
            "unexpected request id index: {request}"
        );
        ensure_index(
            &indexes,
            "ix_audit_events_visibility_created_at",
            "(visibility, created_at DESC)",
            None,
        )?;
        ensure_index(
            &indexes,
            "ix_audit_events_actor_created_at",
            "(actor_user_id, created_at DESC)",
            Some("actor_user_id IS NOT NULL"),
        )?;
        ensure_index(
            &indexes,
            "ix_audit_events_actor_node_created_at",
            "(actor_node_id, created_at DESC)",
            Some("actor_node_id IS NOT NULL"),
        )?;
        ensure_index(&indexes, "ix_audit_events_created_at", "(created_at)", None)?;

        Ok(())
    }

    fn ensure_index(
        indexes: &BTreeMap<String, String>,
        name: &str,
        columns: &str,
        predicate: Option<&str>,
    ) -> anyhow::Result<()> {
        let definition = indexes
            .get(name)
            .with_context(|| format!("missing index {name}"))?;
        ensure!(
            definition.contains(columns),
            "index {name} has unexpected columns: {definition}"
        );
        if let Some(predicate) = predicate {
            ensure!(
                definition.contains(predicate),
                "index {name} has unexpected predicate: {definition}"
            );
        }
        Ok(())
    }

    async fn assert_audit_backfill(
        db: &DatabaseConnection,
        deployment_id: Uuid,
    ) -> anyhow::Result<()> {
        let rows = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT action, actor_type::text AS actor_type, visibility::text AS visibility
FROM audit_events
WHERE action IN (
    'project.updated',
    'project.deleted',
    'deployment.release.completed',
    'team.quota_plan_overridden'
)
ORDER BY action
"#,
            ))
            .await?;
        let backfilled = rows
            .into_iter()
            .map(|row| {
                Ok((
                    row.try_get::<String>("", "action")?,
                    (
                        row.try_get::<String>("", "actor_type")?,
                        row.try_get::<String>("", "visibility")?,
                    ),
                ))
            })
            .collect::<Result<BTreeMap<_, _>, sea_orm::DbErr>>()?;
        ensure!(
            backfilled
                == BTreeMap::from([
                    (
                        "deployment.release.completed".to_owned(),
                        ("user".to_owned(), "platform".to_owned()),
                    ),
                    (
                        "project.deleted".to_owned(),
                        ("system".to_owned(), "platform".to_owned()),
                    ),
                    (
                        "project.updated".to_owned(),
                        ("user".to_owned(), "team".to_owned()),
                    ),
                    (
                        "team.quota_plan_overridden".to_owned(),
                        ("system".to_owned(), "platform".to_owned()),
                    ),
                ]),
            "unexpected audit backfill: {backfilled:#?}"
        );

        let deployment_visibility = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                format!(
                    "SELECT pending_release_audit_visibility::text AS visibility FROM deployments WHERE id = '{deployment_id}'::uuid"
                ),
            ))
            .await?
            .context("seeded pending release deployment is missing")?
            .try_get::<String>("", "visibility")?;
        ensure!(
            deployment_visibility == "platform",
            "pending release backfilled to {deployment_visibility:?} instead of platform"
        );

        Ok(())
    }

    async fn assert_audit_foundation_objects_absent(db: &DatabaseConnection) -> anyhow::Result<()> {
        let column_count = object_count(
            db,
            r#"
SELECT count(*)::bigint AS count
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND (
    (table_name = 'audit_events' AND column_name IN (
      'actor_type', 'actor_node_id', 'visibility', 'request_id', 'source_ip',
      'user_agent', 'http_method', 'request_path', 'status_code', 'duration_ms', 'changes'
    ))
    OR (table_name = 'deployments' AND column_name = 'pending_release_audit_visibility')
  )
"#,
        )
        .await?;
        ensure!(
            column_count == 0,
            "audit foundation columns remained after down migration"
        );

        let enum_count = audit_enum_count(db).await?;
        ensure!(
            enum_count == 0,
            "audit foundation enum types remained after down migration"
        );
        Ok(())
    }

    async fn assert_audit_foundation_objects_restored(
        db: &DatabaseConnection,
    ) -> anyhow::Result<()> {
        let column_count = object_count(
            db,
            r#"
SELECT count(*)::bigint AS count
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND (
    (table_name = 'audit_events' AND column_name IN (
      'actor_type', 'actor_node_id', 'visibility', 'request_id', 'source_ip',
      'user_agent', 'http_method', 'request_path', 'status_code', 'duration_ms', 'changes'
    ))
    OR (table_name = 'deployments' AND column_name = 'pending_release_audit_visibility')
  )
"#,
        )
        .await?;
        ensure!(
            column_count == 12,
            "audit foundation columns were not restored after reapplying migration"
        );

        let enum_count = audit_enum_count(db).await?;
        ensure!(
            enum_count == 2,
            "audit foundation enum types were not restored after reapplying migration"
        );
        Ok(())
    }

    async fn audit_enum_count(db: &DatabaseConnection) -> anyhow::Result<i64> {
        object_count(
            db,
            r#"
SELECT count(*)::bigint AS count
FROM pg_type t
JOIN pg_namespace n ON n.oid = t.typnamespace
WHERE n.nspname = current_schema()
  AND t.typname IN ('audit_actor_type', 'audit_event_visibility')
"#,
        )
        .await
    }

    async fn object_count(db: &DatabaseConnection, sql: &str) -> anyhow::Result<i64> {
        db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
            .await?
            .context("count query returned no row")?
            .try_get::<i64>("", "count")
            .map_err(Into::into)
    }
}
