use super::*;
use axum::{body::to_bytes, response::IntoResponse};
use grass_cache::{CacheBackend, CacheStore};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, Database, DatabaseConnection, EntityTrait,
    QueryFilter,
};
use sea_orm_migration::MigratorTrait;
use tower::ServiceExt;

use crate::{
    domain::{
        deployments::{self, CreateDeploymentParams},
        nodes,
        projects::{self, CreateProjectParams},
        scheduler::{Placement, PlacementMode},
        teams::{self, CreateTeamParams},
    },
    infra::{
        config::ControlApiConfig,
        database::{
            entity::{
                AuditEventResult, DeploymentArtifactKind, DeploymentBuildStatus,
                DeploymentEnvironment, DeploymentReleaseStatus, DeploymentServeStatus,
                NodeConfigSyncStatus, PlatformRole, ProjectRuntime, TeamKind, UserStatus,
                audit_event, deployment_artifact, project, user,
            },
            migrate::Migrator,
        },
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

struct PostgresTestDatabase {
    db: DatabaseConnection,
    admin: DatabaseConnection,
    schema: String,
}

impl PostgresTestDatabase {
    async fn start() -> Option<Self> {
        let database_url = std::env::var("GRASS_TEST_DATABASE_URL").ok()?;
        let admin = Database::connect(&database_url).await.unwrap();
        let schema = format!("gw_node_deletion_{}", Uuid::now_v7().simple());
        admin
            .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
            .await
            .unwrap();
        let mut scoped_url = url::Url::parse(&database_url).unwrap();
        scoped_url
            .query_pairs_mut()
            .append_pair("options", &format!("-csearch_path={schema}"));
        let db = Database::connect(scoped_url.as_str()).await.unwrap();
        let _migration_guard = crate::infra::database::migrate::MIGRATION_TEST_LOCK
            .lock()
            .await;
        Migrator::up(&db, None).await.unwrap();
        Some(Self { db, admin, schema })
    }

    async fn cleanup(self) {
        self.db.close().await.unwrap();
        self.admin
            .execute_unprepared(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .await
            .unwrap();
        self.admin.close().await.unwrap();
    }
}

struct DeletionFixture {
    user: user::Model,
    project: project::Model,
    source: node::Model,
    target: node::Model,
    deployment: deployment::Model,
}

async fn active_node(db: &DatabaseConnection, name: &str) -> node::Model {
    let now = OffsetDateTime::now_utc();
    node::ActiveModel {
        id: Set(Uuid::now_v7()),
        name: Set(name.to_owned()),
        region: Set("default".to_owned()),
        token_hash: Set(format!("token-{name}")),
        status: Set(NodeStatus::Active),
        build_enabled: Set(true),
        serve_enabled: Set(true),
        build_concurrency: Set(2),
        base_url: Set(Some(format!("http://{name}.example.test"))),
        work_root: Set(Some(format!("/data/{name}"))),
        capacity_cpu_millicores: Set(10_000),
        capacity_memory_mb: Set(10_000),
        capacity_disk_mb: Set(10_000),
        max_deployments: Set(20),
        metadata: Set(serde_json::json!({})),
        last_heartbeat_at: Set(Some(now)),
        desired_config: Set(None),
        desired_config_revision: Set(0),
        effective_config: Set(None),
        effective_config_revision: Set(0),
        config_sync_status: Set(NodeConfigSyncStatus::Pending),
        config_sync_error: Set(None),
        node_token_configured: Set(false),
        config_updated_at: Set(None),
        config_applied_at: Set(None),
        deleted_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .unwrap()
}

async fn create_deployment(
    db: &DatabaseConnection,
    project: &project::Model,
    source_node_id: Uuid,
    with_artifact: bool,
    active_release: bool,
) -> deployment::Model {
    let deployment = deployments::create_deployment(
        db,
        CreateDeploymentParams {
            project: project.clone(),
            environment: DeploymentEnvironment::Production,
            triggered_by_user_id: None,
            branch: Some("main".to_owned()),
            commit_hash: None,
            commit_message: None,
            preview_host: None,
            source_credential_version_id: None,
        },
        Placement {
            node_id: source_node_id,
            region: "default".to_owned(),
            overcommitted: false,
            mode: PlacementMode::Automatic,
        },
    )
    .await
    .unwrap();
    let mut active: deployment::ActiveModel = deployment.into();
    active.build_status = Set(DeploymentBuildStatus::Ready);
    active.serve_status = Set(DeploymentServeStatus::Ready);
    active.release_status = Set(if active_release {
        DeploymentReleaseStatus::Active
    } else {
        DeploymentReleaseStatus::Draft
    });
    let deployment = active.update(db).await.unwrap();
    if with_artifact {
        deployment_artifact::ActiveModel {
            id: Set(Uuid::now_v7()),
            deployment_id: Set(deployment.id),
            kind: Set(DeploymentArtifactKind::GrassOutput),
            storage_path: Set(format!("artifacts/{}.zip", deployment.id)),
            checksum_sha256: Set(Some("a".repeat(64))),
            size_bytes: Set(Some(128)),
            manifest: Set(serde_json::json!({ "unpacked_size_bytes": 256 })),
            deleted_at: Set(None),
            created_at: Set(OffsetDateTime::now_utc()),
        }
        .insert(db)
        .await
        .unwrap();
    }
    deployment
}

async fn seed_fixture(db: &DatabaseConnection, with_artifact: bool) -> DeletionFixture {
    let now = OffsetDateTime::now_utc();
    let user = user::ActiveModel {
        auth_version: Set(1),
        id: Set(Uuid::now_v7()),
        email: Set(format!("{}@example.test", Uuid::now_v7().simple())),
        display_name: Set(Some("Node Deletion Tester".to_owned())),
        avatar_version: Set(None),
        status: Set(UserStatus::Active),
        platform_role: Set(PlatformRole::Admin),
        email_verified_at: Set(Some(now)),
        last_login_at: Set(None),
        deleted_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .unwrap();
    let team = teams::create_team(
        db,
        CreateTeamParams {
            slug: format!("team-{}", Uuid::now_v7().simple()),
            name: "Node Deletion Team".to_owned(),
            kind: TeamKind::Team,
            owner_user_id: user.id,
            group_id: None,
        },
    )
    .await
    .unwrap();
    let project = projects::create_project(
        db,
        CreateProjectParams {
            team_id: team.id,
            created_by_user_id: None,
            slug: format!("project-{}", Uuid::now_v7().simple()),
            name: "Node Deletion Project".to_owned(),
            runtime: ProjectRuntime::Static,
            repository_url: None,
            default_branch: Some("main".to_owned()),
            install_command: None,
            build_command: None,
            output_directory: None,
            source_config: serde_json::json!({}),
            build_config: serde_json::json!({}),
        },
    )
    .await
    .unwrap();
    let source = active_node(db, &format!("source-{}", Uuid::now_v7().simple())).await;
    let target = active_node(db, &format!("target-{}", Uuid::now_v7().simple())).await;
    let deployment = create_deployment(db, &project, source.id, with_artifact, true).await;
    DeletionFixture {
        user,
        project,
        source,
        target,
        deployment,
    }
}

async fn enqueue_fixture(db: &DatabaseConnection, fixture: &DeletionFixture) {
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .unwrap();
    scheduler::lock_placement(&transaction).await.unwrap();
    let source = node::Entity::find_by_id(fixture.source.id)
        .one(&transaction)
        .await
        .unwrap()
        .unwrap();
    enqueue(
        &transaction,
        source,
        Some(fixture.target.id),
        fixture.user.id,
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn reload_job(db: &DatabaseConnection, node_id: Uuid) -> node_deletion_job::Model {
    active_job(db, node_id).await.unwrap().unwrap()
}

async fn set_migration_status(
    db: &DatabaseConnection,
    job_id: Uuid,
    status: NodeDeploymentMigrationStatus,
    error: Option<&str>,
) {
    let migration = node_deployment_migration::Entity::find()
        .filter(node_deployment_migration::Column::JobId.eq(job_id))
        .one(db)
        .await
        .unwrap()
        .unwrap();
    let ready = matches!(status, NodeDeploymentMigrationStatus::Ready);
    let mut active: node_deployment_migration::ActiveModel = migration.into();
    active.status = Set(status);
    active.error = Set(error.map(str::to_owned));
    active.ready_at = Set(ready.then(OffsetDateTime::now_utc));
    active.updated_at = Set(OffsetDateTime::now_utc());
    active.update(db).await.unwrap();
}

async fn audit_exists(
    db: &DatabaseConnection,
    node_id: Uuid,
    action: &str,
    result: AuditEventResult,
) -> bool {
    audit_event::Entity::find()
        .filter(audit_event::Column::TargetId.eq(node_id))
        .filter(audit_event::Column::Action.eq(action))
        .filter(audit_event::Column::Result.eq(result))
        .one(db)
        .await
        .unwrap()
        .is_some()
}

#[test]
fn deletion_waits_for_shadow_migrations_then_active_builds() {
    assert_eq!(next_phase(2, 0, 0, 0), DeletionPhase::Migrating);
    assert_eq!(next_phase(2, 1, 0, 0), DeletionPhase::Migrating);
    assert_eq!(next_phase(2, 2, 0, 1), DeletionPhase::Draining);
    assert_eq!(next_phase(2, 2, 0, 0), DeletionPhase::Deleting);
    assert_eq!(next_phase(0, 0, 0, 2), DeletionPhase::Draining);
    assert_eq!(next_phase(0, 0, 0, 0), DeletionPhase::Deleting);
}

#[test]
fn a_failed_shadow_copy_never_advances_to_route_switch_or_deletion() {
    assert_eq!(next_phase(3, 2, 1, 0), DeletionPhase::Failed);
}

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL and disposable schema permission"]
async fn postgres_failed_migration_retries_then_cuts_over_and_drains_builds() {
    let test_db = PostgresTestDatabase::start()
        .await
        .expect("GRASS_TEST_DATABASE_URL is required for this ignored test");
    let fixture = seed_fixture(&test_db.db, true).await;
    let active_build = create_deployment(
        &test_db.db,
        &fixture.project,
        fixture.source.id,
        true,
        false,
    )
    .await;
    let mut active: deployment::ActiveModel = active_build.into();
    active.build_status = Set(DeploymentBuildStatus::Building);
    active.build_node_id = Set(Some(fixture.source.id));
    active.serve_node_id = Set(None);
    active.serve_status = Set(DeploymentServeStatus::Retired);
    let active_build = active.update(&test_db.db).await.unwrap();

    enqueue_fixture(&test_db.db, &fixture).await;
    let job = reload_job(&test_db.db, fixture.source.id).await;
    set_migration_status(
        &test_db.db,
        job.id,
        NodeDeploymentMigrationStatus::Failed,
        Some("shadow copy failed"),
    )
    .await;
    process_pending_jobs(&test_db.db).await.unwrap();
    assert_eq!(
        reload_job(&test_db.db, fixture.source.id).await.status,
        NodeDeletionStatus::Failed
    );
    assert_eq!(
        deployment::Entity::find_by_id(fixture.deployment.id)
            .one(&test_db.db)
            .await
            .unwrap()
            .unwrap()
            .serve_node_id,
        Some(fixture.source.id)
    );
    assert!(
        audit_exists(
            &test_db.db,
            fixture.source.id,
            "node.deletion_failed",
            AuditEventResult::Failure,
        )
        .await
    );

    enqueue_fixture(&test_db.db, &fixture).await;
    assert!(
        audit_exists(
            &test_db.db,
            fixture.source.id,
            "node.deletion_retried",
            AuditEventResult::Success,
        )
        .await
    );
    let job = reload_job(&test_db.db, fixture.source.id).await;
    set_migration_status(
        &test_db.db,
        job.id,
        NodeDeploymentMigrationStatus::Ready,
        None,
    )
    .await;
    process_pending_jobs(&test_db.db).await.unwrap();
    let job = reload_job(&test_db.db, fixture.source.id).await;
    assert_eq!(job.status, NodeDeletionStatus::Draining);
    assert_eq!(job.active_builds, 1);
    assert_eq!(
        deployment::Entity::find_by_id(fixture.deployment.id)
            .one(&test_db.db)
            .await
            .unwrap()
            .unwrap()
            .serve_node_id,
        Some(fixture.target.id)
    );
    assert!(
        audit_exists(
            &test_db.db,
            fixture.source.id,
            "node.deletion_route_switched",
            AuditEventResult::Success,
        )
        .await
    );
    assert!(
        nodes::get_by_id(&test_db.db, fixture.source.id)
            .await
            .unwrap()
            .is_some()
    );

    let mut active: deployment::ActiveModel = active_build.into();
    active.build_status = Set(DeploymentBuildStatus::Ready);
    active.build_finished_at = Set(Some(OffsetDateTime::now_utc()));
    active.update(&test_db.db).await.unwrap();
    process_pending_jobs(&test_db.db).await.unwrap();
    assert_eq!(
        reload_job(&test_db.db, fixture.source.id).await.status,
        NodeDeletionStatus::Deleting
    );
    assert!(
        nodes::get_by_id(&test_db.db, fixture.source.id)
            .await
            .unwrap()
            .is_some()
    );
    process_pending_jobs(&test_db.db).await.unwrap();
    let completed = node_deletion_job::Entity::find_by_id(job.id)
        .one(&test_db.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed.status, NodeDeletionStatus::Completed);
    assert!(
        nodes::get_by_id(&test_db.db, fixture.source.id)
            .await
            .unwrap()
            .is_none()
    );

    test_db.cleanup().await;
}

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL and disposable schema permission"]
async fn postgres_missing_artifact_fails_without_cutover() {
    let test_db = PostgresTestDatabase::start()
        .await
        .expect("GRASS_TEST_DATABASE_URL is required for this ignored test");
    let fixture = seed_fixture(&test_db.db, false).await;
    enqueue_fixture(&test_db.db, &fixture).await;

    process_pending_jobs(&test_db.db).await.unwrap();

    let job = reload_job(&test_db.db, fixture.source.id).await;
    assert_eq!(job.status, NodeDeletionStatus::Failed);
    assert!(
        job.error
            .as_deref()
            .is_some_and(|error| error.contains("artifact"))
    );
    assert_eq!(
        deployment::Entity::find_by_id(fixture.deployment.id)
            .one(&test_db.db)
            .await
            .unwrap()
            .unwrap()
            .serve_node_id,
        Some(fixture.source.id)
    );

    test_db.cleanup().await;
}

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL and disposable schema permission"]
async fn postgres_shadow_ready_reports_do_not_change_the_live_deployment() {
    let test_db = PostgresTestDatabase::start()
        .await
        .expect("GRASS_TEST_DATABASE_URL is required for this ignored test");
    let fixture = seed_fixture(&test_db.db, true).await;
    enqueue_fixture(&test_db.db, &fixture).await;
    let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
    state.database.set(test_db.db.clone()).unwrap();
    for _ in 0..2 {
        let response = crate::features::api::v1::internal::serve::deployments::by_deployment_id::status::router().with_state(state.clone()).oneshot(axum::http::Request::builder().method("POST").uri(format!("/serve/deployments/{}/status", fixture.deployment.id))
       .header("content-type", "application/json")
       .extension(AuthenticatedNode(fixture.target.clone()))
       .body(axum::body::Body::from(serde_json::json!({"status": "ready", "failure_code": null, "failure_message": null}).to_string())).unwrap())
            .await
            .unwrap();
        assert!(response.status().is_success());
    }

    let migration = node_deployment_migration::Entity::find()
        .filter(node_deployment_migration::Column::DeploymentId.eq(fixture.deployment.id))
        .one(&test_db.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(migration.status, NodeDeploymentMigrationStatus::Ready);
    let live = deployment::Entity::find_by_id(fixture.deployment.id)
        .one(&test_db.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(live.serve_node_id, Some(fixture.source.id));
    assert_eq!(live.serve_status, DeploymentServeStatus::Ready);
    assert_eq!(live.release_status, DeploymentReleaseStatus::Active);

    test_db.cleanup().await;
}

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL and disposable schema permission"]
async fn postgres_unhealthy_target_fails_before_route_cutover() {
    let test_db = PostgresTestDatabase::start()
        .await
        .expect("GRASS_TEST_DATABASE_URL is required for this ignored test");
    let fixture = seed_fixture(&test_db.db, true).await;
    enqueue_fixture(&test_db.db, &fixture).await;
    let mut target: node::ActiveModel = fixture.target.clone().into();
    target.status = Set(NodeStatus::Offline);
    target.update(&test_db.db).await.unwrap();

    process_pending_jobs(&test_db.db).await.unwrap();

    let job = reload_job(&test_db.db, fixture.source.id).await;
    assert_eq!(job.status, NodeDeletionStatus::Failed);
    assert!(
        job.error
            .as_deref()
            .is_some_and(|error| error.contains("healthy"))
    );
    assert!(
        node_deployment_migration::Entity::find()
            .filter(node_deployment_migration::Column::JobId.eq(job.id))
            .all(&test_db.db)
            .await
            .unwrap()
            .into_iter()
            .all(|migration| matches!(migration.status, NodeDeploymentMigrationStatus::Failed))
    );
    assert_eq!(
        deployment::Entity::find_by_id(fixture.deployment.id)
            .one(&test_db.db)
            .await
            .unwrap()
            .unwrap()
            .serve_node_id,
        Some(fixture.source.id)
    );

    test_db.cleanup().await;
}

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL and disposable schema permission"]
async fn postgres_stale_active_snapshot_cannot_claim_after_node_starts_draining() {
    let test_db = PostgresTestDatabase::start()
        .await
        .expect("GRASS_TEST_DATABASE_URL is required for this ignored test");
    crate::infra::database::seed::run(&test_db.db)
        .await
        .unwrap();
    let fixture = seed_fixture(&test_db.db, true).await;
    let pending = deployments::create_deployment(
        &test_db.db,
        CreateDeploymentParams {
            project: fixture.project.clone(),
            environment: DeploymentEnvironment::Preview,
            triggered_by_user_id: None,
            branch: Some("claim-race".to_owned()),
            commit_hash: None,
            commit_message: None,
            preview_host: None,
            source_credential_version_id: None,
        },
        Placement {
            node_id: fixture.source.id,
            region: "default".to_owned(),
            overcommitted: false,
            mode: PlacementMode::Automatic,
        },
    )
    .await
    .unwrap();
    let authenticated_snapshot = fixture.source.clone();
    let mut source: node::ActiveModel = fixture.source.clone().into();
    source.status = Set(NodeStatus::Draining);
    source.update(&test_db.db).await.unwrap();
    let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
    state.database.set(test_db.db.clone()).unwrap();
    assert!(
        state
            .cache
            .set(
                CacheStore::connect_cache(CacheBackend::Moka, "")
                    .await
                    .unwrap(),
            )
            .is_ok()
    );

    let response = crate::features::api::v1::internal::deployments::claim::router()
        .with_state(state)
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/deployments/claim")
                .header("content-type", "application/json")
                .extension(AuthenticatedNode(authenticated_snapshot))
                .body(axum::body::Body::from(r#"{"capacity":1}"#))
                .unwrap(),
        )
        .await
        .unwrap()
        .into_response();
    assert!(response.status().is_success());
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(body["data"]["deployment"].is_null());
    let pending = deployment::Entity::find_by_id(pending.id)
        .one(&test_db.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.build_status, DeploymentBuildStatus::Pending);
    assert_eq!(pending.build_node_id, None);

    test_db.cleanup().await;
}

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL and disposable schema permission"]
async fn postgres_plan_requires_whole_batch_capacity_and_zero_work_has_deleting_phase() {
    let test_db = PostgresTestDatabase::start()
        .await
        .expect("GRASS_TEST_DATABASE_URL is required for this ignored test");
    let fixture = seed_fixture(&test_db.db, true).await;
    create_deployment(
        &test_db.db,
        &fixture.project,
        fixture.source.id,
        true,
        false,
    )
    .await;
    let mut target: node::ActiveModel = fixture.target.clone().into();
    target.capacity_cpu_millicores = Set(50);
    target.capacity_memory_mb = Set(64);
    target.capacity_disk_mb = Set(256);
    target.max_deployments = Set(1);
    target.update(&test_db.db).await.unwrap();
    let plan = plan(&test_db.db, &fixture.source).await.unwrap();
    assert!(plan.requires_target);
    assert_eq!(plan.assigned_deployments, 2);
    assert!(plan.eligible_targets.is_empty());

    let mut first: deployment::ActiveModel = fixture.deployment.clone().into();
    first.build_status = Set(DeploymentBuildStatus::Failed);
    first.update(&test_db.db).await.unwrap();
    deployment::Entity::update_many()
        .col_expr(
            deployment::Column::BuildStatus,
            sea_orm::ActiveEnum::as_enum(&DeploymentBuildStatus::Failed),
        )
        .filter(deployment::Column::ServeNodeId.eq(fixture.source.id))
        .exec(&test_db.db)
        .await
        .unwrap();
    let source = node::Entity::find_by_id(fixture.source.id)
        .one(&test_db.db)
        .await
        .unwrap()
        .unwrap();
    let transaction = crate::infra::audit::AuditTransaction::begin(&test_db.db)
        .await
        .unwrap();
    scheduler::lock_placement(&transaction).await.unwrap();
    enqueue(&transaction, source, None, fixture.user.id)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    process_pending_jobs(&test_db.db).await.unwrap();
    assert_eq!(
        reload_job(&test_db.db, fixture.source.id).await.status,
        NodeDeletionStatus::Deleting
    );
    assert!(
        nodes::get_by_id(&test_db.db, fixture.source.id)
            .await
            .unwrap()
            .is_some()
    );
    process_pending_jobs(&test_db.db).await.unwrap();
    assert!(
        nodes::get_by_id(&test_db.db, fixture.source.id)
            .await
            .unwrap()
            .is_none()
    );

    test_db.cleanup().await;
}
