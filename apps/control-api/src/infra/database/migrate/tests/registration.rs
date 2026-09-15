use std::future::Future;

use sea_orm_migration::MigratorTrait;
use tokio::sync::oneshot;

use super::super::{MIGRATION_TEST_LOCK, Migrator, migration};

#[test]
fn registers_audit_foundation_migration() {
    let migrations = Migrator::migrations();

    assert_eq!(migrations.len(), 35);
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

    assert_eq!(migrations.len(), 35);
    assert_eq!(
        migrations.get(12).expect("thirteenth migration").name(),
        "m20260729_000013_team_group_review_policy"
    );
}

#[test]
fn registers_node_config_sync_migration() {
    let migrations = Migrator::migrations();

    assert_eq!(migrations.len(), 35);
    assert_eq!(
        migrations.get(13).expect("fourteenth migration").name(),
        "m20260729_000014_node_config_sync"
    );
}

#[test]
fn registers_node_deletion_queue_migration() {
    let migrations = Migrator::migrations();

    assert_eq!(migrations.len(), 35);
    assert_eq!(
        migrations.get(14).expect("fifteenth migration").name(),
        "m20260729_000015_node_deletion_queue"
    );
}

#[test]
fn registers_domain_review_policy_after_node_deletion_queue() {
    let migrations = Migrator::migrations();

    assert_eq!(migrations.len(), 35);
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

    assert_eq!(migrations.len(), 35);
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
        migrations.get(22).expect("twenty-third migration").name(),
        "m20260804_000023_mfa_policy"
    );
}

#[test]
fn registers_scoped_codes_after_authentication_migrations() {
    let migrations = Migrator::migrations();

    assert_eq!(migrations.len(), 35);
    assert_eq!(
        migrations.get(23).expect("twenty-fourth migration").name(),
        "m20260806_000024_scoped_codes"
    );
}

#[test]
fn registers_registration_allowlist_after_scoped_codes() {
    let migrations = Migrator::migrations();

    assert_eq!(migrations.len(), 35);
    assert_eq!(
        migrations.get(24).expect("twenty-fifth migration").name(),
        "m20260806_000025_registration_allowlist"
    );
}

#[test]
fn registers_avatar_versions_after_registration_allowlist() {
    let migrations = Migrator::migrations();

    assert_eq!(migrations.len(), 35);
    assert_eq!(
        migrations.get(25).expect("twenty-sixth migration").name(),
        "m20260807_000026_avatars"
    );
}

#[test]
fn registers_object_storage_after_deployment_screenshots() {
    let migrations = Migrator::migrations();

    assert_eq!(migrations.len(), 35);
    assert_eq!(
        migrations.get(26).expect("twenty-seventh migration").name(),
        "m20260807_000027_deployment_screenshots"
    );
    assert_eq!(
        migrations.get(27).expect("twenty-eighth migration").name(),
        "m20260808_000028_object_storage"
    );
    assert_eq!(
        migrations.last().expect("last migration").name(),
        "m20260912_000035_user_auth_version"
    );
}

#[tokio::test]
async fn shared_postgres_migration_lock_serializes_access() {
    let (started_sender, started_receiver) = oneshot::channel();
    let (release_sender, release_receiver) = oneshot::channel();
    let first = tokio::spawn(async move {
        let _guard = MIGRATION_TEST_LOCK.lock().await;
        started_sender.send(()).unwrap();
        release_receiver.await.unwrap();
    });
    started_receiver.await.unwrap();

    let (second_attempted_sender, second_attempted_receiver) = oneshot::channel();
    let (second_acquired_sender, mut second_acquired_receiver) = oneshot::channel();
    let second = tokio::spawn(async move {
        let mut lock = Box::pin(MIGRATION_TEST_LOCK.lock());
        let mut attempted_sender = Some(second_attempted_sender);
        let _guard = std::future::poll_fn(move |context| {
            if let Some(sender) = attempted_sender.take() {
                sender.send(()).unwrap();
            }
            lock.as_mut().poll(context)
        })
        .await;
        second_acquired_sender.send(()).unwrap();
    });

    second_attempted_receiver.await.unwrap();
    assert!(second_acquired_receiver.try_recv().is_err());

    release_sender.send(()).unwrap();
    second.await.unwrap();
    assert!(second_acquired_receiver.await.is_ok());
    first.await.unwrap();
}
