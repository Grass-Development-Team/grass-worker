use std::collections::BTreeMap;

use anyhow::{Context, ensure};
use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;
use uuid::Uuid;

use super::super::Migrator;

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ColumnShape {
    pub(super) name: String,
    pub(super) udt_name: String,
    pub(super) nullable: String,
    pub(super) default: Option<String>,
}

pub(super) struct PostgresMigrationDatabase {
    pub(super) db: DatabaseConnection,
    admin: DatabaseConnection,
    schema: String,
}

impl PostgresMigrationDatabase {
    pub(super) async fn start(database_url: &str) -> anyhow::Result<Self> {
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

    pub(super) async fn cleanup(self) -> anyhow::Result<()> {
        self.db.close().await?;
        self.admin
            .execute_unprepared(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .await?;
        self.admin.close().await?;
        Ok(())
    }
}

pub(super) async fn assert_migration_tracking(
    db: &DatabaseConnection,
    applied_count: usize,
) -> anyhow::Result<()> {
    let applied = Migrator::get_applied_migrations(db).await?;
    let pending = Migrator::get_pending_migrations(db).await?;
    // Historical shape tests stop at their target migration. Later migrations
    // remain pending even when that historical phase is fully applied.
    let pending_count = Migrator::migrations().len() - applied_count;

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

pub(super) fn column(
    name: &str,
    udt_name: &str,
    nullable: &str,
    default: Option<&str>,
) -> ColumnShape {
    ColumnShape {
        name: name.to_owned(),
        udt_name: udt_name.to_owned(),
        nullable: nullable.to_owned(),
        default: default.map(str::to_owned),
    }
}

pub(super) async fn query_column_shapes(
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

pub(super) fn ensure_index(
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

pub(super) async fn object_count(db: &DatabaseConnection, sql: &str) -> anyhow::Result<i64> {
    db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
        .await?
        .context("count query returned no row")?
        .try_get::<i64>("", "count")
        .map_err(Into::into)
}
