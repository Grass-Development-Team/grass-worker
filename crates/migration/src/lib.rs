mod m20260416_000001_create_core_tables;
mod m20260417_000002_add_initial_admin_flag;
mod m20260423_000003_expand_project_lifecycle;
mod m20260501_000004_add_project_active_deployment;

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
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrator_registers_initial_migration() {
        let migrations = Migrator::migrations();

        assert_eq!(migrations.len(), 4);
        assert_eq!(migrations[0].name(), "m20260416_000001_create_core_tables");
    }

    #[test]
    fn migrator_registers_initial_admin_flag_migration() {
        let migrations = Migrator::migrations();

        assert_eq!(migrations.len(), 4);
        assert_eq!(
            migrations[1].name(),
            "m20260417_000002_add_initial_admin_flag"
        );
    }

    #[test]
    fn migrator_registers_project_lifecycle_migration() {
        let migrations = Migrator::migrations();

        assert_eq!(migrations.len(), 4);
        assert_eq!(
            migrations[2].name(),
            "m20260423_000003_expand_project_lifecycle"
        );
    }

    #[test]
    fn migrator_registers_project_active_deployment_migration() {
        let migrations = Migrator::migrations();

        assert_eq!(migrations.len(), 4);
        assert_eq!(
            migrations[3].name(),
            "m20260501_000004_add_project_active_deployment"
        );
    }
}
