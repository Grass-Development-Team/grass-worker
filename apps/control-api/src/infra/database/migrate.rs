use super::migration;
use sea_orm::DatabaseConnection;
use sea_orm_migration::{MigratorTrait, prelude::*};

#[cfg(test)]
pub(crate) static MIGRATION_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
            Box::new(migration::m20260804_000023_mfa_policy::Migration),
            Box::new(migration::m20260806_000024_scoped_codes::Migration),
            Box::new(migration::m20260806_000025_registration_allowlist::Migration),
            Box::new(migration::m20260807_000026_avatars::Migration),
            Box::new(migration::m20260807_000027_deployment_screenshots::Migration),
            Box::new(migration::m20260808_000028_object_storage::Migration),
            Box::new(migration::m20260908_000029_regional_routing::Migration),
            Box::new(migration::m20260908_000030_regional_ingress::Migration),
            Box::new(migration::m20260909_000031_regional_ingress_lifecycle::Migration),
            Box::new(migration::m20260910_000032_managed_certificates::Migration),
            Box::new(migration::m20260911_000033_regions::Migration),
            Box::new(migration::m20260911_000034_domain_onboarding::Migration),
            Box::new(migration::m20260912_000035_user_auth_version::Migration),
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
mod tests;
