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
        ]
    }
}

pub async fn run(database: &DatabaseConnection) -> anyhow::Result<()> {
    Migrator::up(database, None)
        .await
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("failed to run database migrations: {error}"))
}
