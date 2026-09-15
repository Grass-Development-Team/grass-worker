use std::collections::BTreeMap;

use anyhow::{Context, ensure};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;
use uuid::Uuid;

use super::super::{MIGRATION_TEST_LOCK, Migrator};
use super::support::{
    PostgresMigrationDatabase, assert_migration_tracking, column, ensure_index, object_count,
    query_column_shapes,
};

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL"]
async fn postgres_audit_foundation_migration_upgrades_v11_and_is_reversible() -> anyhow::Result<()>
{
    let _migration_guard = MIGRATION_TEST_LOCK.lock().await;
    let database_url = std::env::var("GRASS_TEST_DATABASE_URL")
        .expect("GRASS_TEST_DATABASE_URL must be set to run this ignored migration test");
    let test_db = PostgresMigrationDatabase::start(&database_url).await?;

    let verification = verify_audit_foundation_migration(&test_db.db).await;
    let cleanup = test_db.cleanup().await;

    match (verification, cleanup) {
        (Err(verification_error), Err(cleanup_error)) => Err(verification_error.context(format!(
            "disposable schema cleanup also failed: {cleanup_error:#}"
        ))),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

async fn verify_audit_foundation_migration(db: &DatabaseConnection) -> anyhow::Result<()> {
    Migrator::up(db, Some(11)).await?;
    assert_migration_tracking(db, 11).await?;

    let user_id = Uuid::now_v7();
    let team_id = Uuid::now_v7();
    let project_id = Uuid::now_v7();
    let deployment_id = Uuid::now_v7();
    seed_v11_audit_fixtures(db, user_id, team_id, project_id, deployment_id).await?;

    Migrator::up(db, Some(1)).await?;
    assert_migration_tracking(db, 12).await?;
    assert_audit_enum_shapes(db).await?;
    assert_audit_column_shapes(db).await?;
    assert_audit_constraints(db).await?;
    assert_audit_indexes(db).await?;
    assert_audit_backfill(db, deployment_id).await?;

    Migrator::down(db, Some(1)).await?;
    assert_migration_tracking(db, 11).await?;
    assert_audit_foundation_objects_absent(db).await?;

    Migrator::up(db, None).await?;
    assert_migration_tracking(db, 35).await?;
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

async fn assert_audit_backfill(db: &DatabaseConnection, deployment_id: Uuid) -> anyhow::Result<()> {
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

async fn assert_audit_foundation_objects_restored(db: &DatabaseConnection) -> anyhow::Result<()> {
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
