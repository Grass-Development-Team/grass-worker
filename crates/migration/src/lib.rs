mod m20260416_000001_create_core_tables;
mod m20260417_000002_add_initial_admin_flag;
mod m20260423_000003_expand_project_lifecycle;
mod m20260501_000004_add_project_active_deployment;
mod m20260503_000005_add_host_binding_models;

pub use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260416_000001_create_core_tables::Migration),
            Box::new(m20260417_000002_add_initial_admin_flag::Migration),
            Box::new(m20260423_000003_expand_project_lifecycle::Migration),
            Box::new(m20260501_000004_add_project_active_deployment::Migration),
            Box::new(m20260503_000005_add_host_binding_models::Migration),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::sea_orm::{
        DatabaseBackend, DatabaseConnection, MockDatabase, MockExecResult,
    };
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    fn block_on<F: Future>(future: F) -> F::Output {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = std::pin::pin!(future);

        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    fn sql_statements(database: DatabaseConnection) -> Vec<String> {
        database
            .into_transaction_log()
            .iter()
            .flat_map(|entry| entry.statements().iter())
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>()
    }

    fn mock_exec_results(count: usize) -> Vec<MockExecResult> {
        (0..count)
            .map(|_| MockExecResult {
                last_insert_id: 0,
                rows_affected: 0,
            })
            .collect::<Vec<_>>()
    }

    #[test]
    fn migrator_registers_initial_migration() {
        let migrations = Migrator::migrations();

        assert_eq!(migrations.len(), 5);
        assert_eq!(migrations[0].name(), "m20260416_000001_create_core_tables");
    }

    #[test]
    fn migrator_registers_initial_admin_flag_migration() {
        let migrations = Migrator::migrations();

        assert_eq!(migrations.len(), 5);
        assert_eq!(
            migrations[1].name(),
            "m20260417_000002_add_initial_admin_flag"
        );
    }

    #[test]
    fn migrator_registers_project_lifecycle_migration() {
        let migrations = Migrator::migrations();

        assert_eq!(migrations.len(), 5);
        assert_eq!(
            migrations[2].name(),
            "m20260423_000003_expand_project_lifecycle"
        );
    }

    #[test]
    fn migrator_registers_project_active_deployment_migration() {
        let migrations = Migrator::migrations();

        assert_eq!(migrations.len(), 5);
        assert_eq!(
            migrations[3].name(),
            "m20260501_000004_add_project_active_deployment"
        );
    }

    #[test]
    fn migrator_registers_host_binding_models_migration() {
        let names = Migrator::migrations()
            .into_iter()
            .map(|migration| migration.name().to_owned())
            .collect::<Vec<_>>();

        assert!(names.contains(
            &"m20260503_000005_add_host_binding_models".to_owned()
        ));
    }

    #[test]
    fn host_binding_models_migration_up_uses_underscore_partial_index_name() {
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_exec_results(mock_exec_results(5))
            .into_connection();
        let manager = SchemaManager::new(&database);

        block_on(m20260503_000005_add_host_binding_models::Migration.up(&manager)).unwrap();
        drop(manager);

        let statements = sql_statements(database);

        assert!(
            statements
                .iter()
                .any(|statement| statement.contains(r#"CREATE TABLE IF NOT EXISTS "platform_host_sources""#))
        );
        assert!(
            statements
                .iter()
                .any(|statement| statement.contains(r#"CREATE TABLE IF NOT EXISTS "project_host_bindings""#))
        );
        assert!(
            statements.iter().any(|statement| statement.contains(
                "CREATE UNIQUE INDEX uq_project_host_bindings_primary_per_project"
            ))
        );
        assert!(statements.iter().all(|statement| !statement.contains(
            "uq-project-host-bindings-primary-per-project"
        )));
    }

    #[test]
    fn host_binding_models_migration_down_uses_matching_underscore_partial_index_name() {
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_exec_results(mock_exec_results(3))
            .into_connection();
        let manager = SchemaManager::new(&database);

        block_on(m20260503_000005_add_host_binding_models::Migration.down(&manager)).unwrap();
        drop(manager);

        let statements = sql_statements(database);

        assert!(statements.iter().any(|statement| statement.contains(
            r#"DROP INDEX IF EXISTS "uq_project_host_bindings_primary_per_project""#
        )));
        assert!(statements.iter().all(|statement| !statement.contains(
            "uq-project-host-bindings-primary-per-project"
        )));
    }
}
