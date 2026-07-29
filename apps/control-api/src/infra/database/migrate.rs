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

        assert_eq!(migrations.len(), 14);
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

    async fn verify_audit_foundation_migration(db: &DatabaseConnection) -> anyhow::Result<()> {
        Migrator::up(db, Some(11)).await?;
        assert_migration_tracking(db, 11, 3).await?;

        let user_id = Uuid::now_v7();
        let team_id = Uuid::now_v7();
        let project_id = Uuid::now_v7();
        let deployment_id = Uuid::now_v7();
        seed_v11_audit_fixtures(db, user_id, team_id, project_id, deployment_id).await?;

        Migrator::up(db, Some(1)).await?;
        assert_migration_tracking(db, 12, 2).await?;
        assert_audit_enum_shapes(db).await?;
        assert_audit_column_shapes(db).await?;
        assert_audit_constraints(db).await?;
        assert_audit_indexes(db).await?;
        assert_audit_backfill(db, deployment_id).await?;

        Migrator::down(db, Some(1)).await?;
        assert_migration_tracking(db, 11, 3).await?;
        assert_audit_foundation_objects_absent(db).await?;

        Migrator::up(db, None).await?;
        assert_migration_tracking(db, 14, 0).await?;
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
