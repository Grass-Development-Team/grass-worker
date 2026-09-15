use super::super::{MIGRATION_TEST_LOCK, Migrator};
use super::support::{
    ColumnShape, PostgresMigrationDatabase, assert_migration_tracking, column, query_column_shapes,
};
use anyhow::{Context, ensure};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;
use std::collections::BTreeMap;
use uuid::Uuid;

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
        assert_migration_tracking(&test_db.db, 19).await?;

        Migrator::up(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 20).await?;
        assert_notification_content_schema(&test_db.db).await?;

        Migrator::up(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 21).await?;
        assert_announcement_schema(&test_db.db).await?;

        Migrator::down(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 20).await?;
        assert_announcement_schema_absent(&test_db.db).await?;
        assert_notification_content_schema(&test_db.db).await?;

        Migrator::up(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 21).await?;
        assert_announcement_schema(&test_db.db).await
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
async fn postgres_project_notification_schema_backfills_and_is_reversible() -> anyhow::Result<()> {
    let _migration_guard = MIGRATION_TEST_LOCK.lock().await;
    let database_url = std::env::var("GRASS_TEST_DATABASE_URL")
        .expect("GRASS_TEST_DATABASE_URL must be set to run this ignored migration test");
    let test_db = PostgresMigrationDatabase::start(&database_url).await?;

    let verification = async {
        Migrator::up(&test_db.db, Some(16)).await?;
        assert_migration_tracking(&test_db.db, 16).await?;
        let (user_id, project_id) = seed_project_notification_fixture(&test_db.db).await?;

        Migrator::up(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 17).await?;
        assert_project_notification_schema(&test_db.db, user_id, project_id).await?;

        Migrator::down(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 16).await?;
        assert_project_notification_schema_absent(&test_db.db).await?;

        Migrator::up(&test_db.db, Some(1)).await?;
        assert_migration_tracking(&test_db.db, 17).await?;
        assert_project_notification_schema(&test_db.db, user_id, project_id).await
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
    ensure!(constraints["fk_user_notifications_recipient_user_id"].contains("ON DELETE CASCADE"));
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

async fn assert_project_notification_schema_absent(db: &DatabaseConnection) -> anyhow::Result<()> {
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
