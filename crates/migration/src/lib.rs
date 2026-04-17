mod m20260416_000001_create_core_tables;
mod m20260417_000002_add_initial_admin_flag;

pub use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260416_000001_create_core_tables::Migration),
            Box::new(m20260417_000002_add_initial_admin_flag::Migration),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrator_registers_initial_migration() {
        let migrations = Migrator::migrations();

        assert_eq!(migrations.len(), 2);
        assert_eq!(migrations[0].name(), "m20260416_000001_create_core_tables");
    }

    #[test]
    fn migrator_registers_initial_admin_flag_migration() {
        let migrations = Migrator::migrations();

        assert_eq!(migrations.len(), 2);
        assert_eq!(
            migrations[1].name(),
            "m20260417_000002_add_initial_admin_flag"
        );
    }
}
