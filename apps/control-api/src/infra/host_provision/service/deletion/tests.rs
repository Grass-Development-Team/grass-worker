use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use grass_cache::{Cache, CacheStore, MokaCache};
use sea_orm::{DbBackend, DbErr, MockDatabase, Value};
use time::OffsetDateTime;

use super::*;
use crate::infra::database::entity::{
    HostBindingEnvironment, HostBindingKind, HostBindingStatus, HostProvisionEventStatus,
    HostReviewStatus, HostSourceKind, NodeConfigSyncStatus, NodeStatus, QuotaEventKind,
    host_provision_event, host_source, node, project_host_binding, quota_event,
    quota_usage_counter,
};

fn binding() -> project_host_binding::Model {
    let now = OffsetDateTime::UNIX_EPOCH;
    project_host_binding::Model {
        id: Uuid::now_v7(),
        project_id: Uuid::now_v7(),
        team_id: Uuid::now_v7(),
        host_source_id: None,
        host: "site.example.invalid".to_owned(),
        region: "default".to_owned(),
        kind: HostBindingKind::Custom,
        environment: HostBindingEnvironment::Production,
        status: HostBindingStatus::Active,
        failure_reason: None,
        is_primary: true,
        review_status: HostReviewStatus::Approved,
        reviewed_by_user_id: None,
        reviewed_at: None,
        review_reason: None,
        ownership_status: "verified".to_owned(),
        ownership_checked_at: Some(now),
        ownership_error: None,
        deleted_at: None,
        created_at: now,
        updated_at: now,
    }
}

fn with_quota(db: MockDatabase, binding: &project_host_binding::Model) -> MockDatabase {
    let counter = quota_usage_counter::Model {
        id: Uuid::now_v7(),
        team_id: binding.team_id,
        dimension: "hosts".to_owned(),
        used_value: 1,
        period_start: None,
        period_end: None,
        updated_at: OffsetDateTime::UNIX_EPOCH,
    };
    db.append_query_results([[quota_event::Model {
        id: Uuid::now_v7(),
        team_id: binding.team_id,
        dimension: "hosts".to_owned(),
        kind: QuotaEventKind::Release,
        delta_value: -1,
        idempotency_key: None,
        resource_type: Some("project_host_binding".to_owned()),
        resource_id: Some(binding.id),
        metadata: json!({}),
        created_at: OffsetDateTime::UNIX_EPOCH,
    }]])
    .append_query_results([[counter.clone()]])
    .append_query_results([[quota_usage_counter::Model {
        used_value: 0,
        ..counter
    }]])
    .append_query_results([[BTreeMap::from([("num_items", Value::BigInt(Some(0)))])]])
}

#[tokio::test]
async fn repeated_deletion_uses_the_persisted_generation_and_restore_starts_a_new_one() {
    let active = binding();
    let first = OffsetDateTime::from_unix_timestamp_nanos(1_234_000).unwrap();
    let second = first + time::Duration::seconds(1);
    let cache = CacheStore::Moka(MokaCache::connect());
    for (already_deleted, generation) in [(false, first), (true, first), (false, second)] {
        let deleted = project_host_binding::Model {
            deleted_at: Some(generation),
            is_primary: false,
            ..active.clone()
        };
        let mut db =
            MockDatabase::new(DbBackend::Postgres).append_query_results([[if already_deleted {
                deleted.clone()
            } else {
                active.clone()
            }]]);
        if !already_deleted {
            db = db.append_query_results([[deleted.clone()]]);
        }
        let db = with_quota(db, &deleted)
            .append_query_results([Vec::<BTreeMap<String, Value>>::new()])
            .into_connection();
        // Retrying as an administrator must not insert another audit or notification.
        let scope = if already_deleted {
            DeleteHostScope::Platform {
                actor_user_id: Uuid::now_v7(),
                reason: None,
            }
        } else {
            DeleteHostScope::Project(active.project_id)
        };
        HostBindingService::new(&db, &cache, "test-key")
            .delete_host("test.delete", active.id, scope)
            .await
            .unwrap();
        assert_eq!(
            cache
                .get(&format!("quota:team:{}:hosts", active.team_id))
                .await
                .unwrap()
                .as_deref(),
            Some("0")
        );
        let statements = format!("{:?}", db.into_transaction_log());
        let key = format!(
            "release:project_host_binding:{}:hosts:generation:{}",
            active.id,
            generation.unix_timestamp_nanos()
        );
        assert!(statements.contains(&key), "{statements}");
        assert!(!statements.contains("audit_events"), "{statements}");
        assert!(!statements.contains("notifications"), "{statements}");
        assert_eq!(
            statements.contains("UPDATE \\\"project_host_bindings\\\""),
            !already_deleted,
            "{statements}"
        );
    }
}

#[tokio::test]
async fn another_projects_binding_is_rejected_before_any_write() {
    let binding = binding();
    let db = MockDatabase::new(DbBackend::Postgres)
        .append_query_results([[binding.clone()]])
        .into_connection();
    let cache = CacheStore::Moka(MokaCache::connect());
    let result = HostBindingService::new(&db, &cache, "test-key")
        .delete_host(
            "test.delete",
            binding.id,
            DeleteHostScope::Project(Uuid::now_v7()),
        )
        .await;
    assert!(matches!(result, Err(AppError::NotFound { .. })));
    let statements = format!("{:?}", db.into_transaction_log());
    assert!(
        !statements.contains("sql: \"UPDATE ") && !statements.contains("INSERT INTO"),
        "{statements}"
    );
}

fn serve_node(base_url: String) -> node::Model {
    let now = OffsetDateTime::UNIX_EPOCH;
    node::Model {
        id: Uuid::now_v7(),
        name: "entry".to_owned(),
        region: "default".to_owned(),
        token_hash: "unused".to_owned(),
        status: NodeStatus::Active,
        build_enabled: false,
        serve_enabled: true,
        build_concurrency: 0,
        base_url: Some(base_url),
        work_root: None,
        capacity_cpu_millicores: 0,
        capacity_memory_mb: 0,
        capacity_disk_mb: 0,
        max_deployments: 0,
        metadata: json!({}),
        last_heartbeat_at: Some(now),
        desired_config: None,
        desired_config_revision: 0,
        effective_config: None,
        effective_config_revision: 0,
        config_sync_status: NodeConfigSyncStatus::Applied,
        config_sync_error: None,
        node_token_configured: false,
        config_updated_at: None,
        config_applied_at: None,
        deleted_at: None,
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn dns_failure_still_releases_quota_and_notifies_routes() {
    // Cover both a recorded provider failure and an infrastructure failure
    // while recording it. A committed tombstone must be withdrawn in either case.
    for recording_fails in [false, true] {
        let received = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new().route(
            "/_grass/internal/routes/invalidate",
            post({
                let received = received.clone();
                move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
                    let received = received.clone();
                    async move {
                        received.lock().unwrap().push((headers, body));
                        StatusCode::OK
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let now = OffsetDateTime::UNIX_EPOCH;
        let source = host_source::Model {
            id: Uuid::now_v7(),
            kind: HostSourceKind::DnsProvider,
            label: "invalid provider configuration".to_owned(),
            base_domain: "example.invalid".to_owned(),
            region: "default".to_owned(),
            enabled: true,
            allows_auto_assign: true,
            is_default: true,
            provider: Some("route53".to_owned()),
            config: json!({}),
            deleted_at: Some(now),
            created_at: now,
            updated_at: now,
        };
        let binding = project_host_binding::Model {
            host_source_id: Some(source.id),
            kind: HostBindingKind::Platform,
            deleted_at: Some(now),
            ..binding()
        };
        let mut db = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([[binding.clone()]])
            .append_query_results([[source.clone()]]);
        db = if recording_fails {
            db.append_query_errors([DbErr::Custom("event storage unavailable".to_owned())])
        } else {
            db.append_query_results([[host_provision_event::Model {
                id: Uuid::now_v7(),
                host_binding_id: binding.id,
                host_source_id: Some(source.id),
                status: HostProvisionEventStatus::Failed,
                operation: "host.deprovision".to_owned(),
                provider_request_id: None,
                error_code: Some("deprovision_failed".to_owned()),
                error_message: Some("provider failure".to_owned()),
                metadata: json!({}),
                created_at: now,
            }]])
        };
        let deployment_id = Uuid::now_v7();
        let db = with_quota(db, &binding)
            .append_query_results([[BTreeMap::from([("id", Value::Uuid(Some(deployment_id)))])]])
            .append_query_results([[serve_node(format!("http://{address}"))]])
            .append_query_results([Vec::<node::Model>::new()])
            .into_connection();
        let cache = CacheStore::Moka(MokaCache::connect());
        let result = HostBindingService::new(&db, &cache, "test-key")
            .delete_host(
                "test.delete",
                binding.id,
                DeleteHostScope::Project(binding.project_id),
            )
            .await;
        assert_eq!(result.is_err(), recording_fails, "{result:?}");
        assert_eq!(
            cache
                .get(&format!("quota:team:{}:hosts", binding.team_id))
                .await
                .unwrap()
                .as_deref(),
            Some("0")
        );
        let received = received.lock().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].1["deployment_id"], deployment_id.to_string());
        assert_eq!(received[0].0["x-grass-gateway-hop"], "1");
        assert_eq!(
            received[0].0["x-grass-gateway-token"],
            crate::domain::nodes::gateway_token("test-key")
        );
        server.abort();
    }
}
