use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{AuditActorType, AuditEventResult, AuditEventVisibility};

use super::{
    AuditEventFilter, CreateAuditEventParams, CreateRequestAuditEventParams, audit_event_query,
    create_audit_event, create_platform_audit_event_with_changes, create_request_audit_event,
    delete_events, domain_actor_type, list_events, prepare_request_event, prune_events_before,
    redact_json, visibility_for_domain_event,
};

#[test]
fn audit_query_supports_platform_filters_and_team_allowlist() {
    use sea_orm::{DbBackend, QueryTrait};

    let actor_id = Uuid::now_v7();
    let target_id = Uuid::now_v7();
    let team_id = Uuid::now_v7();
    let filter = AuditEventFilter {
        action: Some("deployment.".to_owned()),
        actor_user_id: Some(actor_id),
        actor_type: Some(AuditActorType::User),
        target_type: Some("deployment".to_owned()),
        target_id: Some(target_id),
        team_id: Some(team_id),
        result: Some(AuditEventResult::Denied),
        created_from: Some(OffsetDateTime::UNIX_EPOCH),
        created_to: Some(OffsetDateTime::UNIX_EPOCH + time::Duration::days(1)),
        visibility: Some(AuditEventVisibility::Team),
        team_visible_only: true,
        page: 2,
        per_page: 25,
    };

    let sql = audit_event_query(&filter)
        .build(DbBackend::Postgres)
        .to_string();

    for fragment in [
        "actor_user_id",
        "actor_type",
        "target_type",
        "target_id",
        "team_id",
        "result",
        "created_at",
        "visibility",
        "deployment.",
        "artifact.uploaded",
        "host.",
        "project.",
        "quota.",
    ] {
        assert!(sql.contains(fragment), "missing {fragment} in {sql}");
    }
}

#[test]
fn audit_values_recursively_redact_secrets() {
    let value = json!({
        "name": "Production",
        "password": "correct horse battery staple",
        "nested": {
            "authorization": "Bearer access-token",
            "database_url": "postgres://admin:secret@db.example/grass",
            "public_key": "ssh-ed25519 AAAA-public",
            "passphrase": "open sesame",
            "api_key": "api-secret",
            "access_key": "access-secret",
            "client_secret": "client-secret",
            "signing_key": "signing-secret",
        },
        "repository_url": "https://example.com/team/repository.git",
        "remote": "https://alice:secret@example.com/private.git",
        "callback_url": "https://example.com/callback?state=public&access_token=hidden",
        "private_key_pem": "-----BEGIN PRIVATE KEY-----\nsecret-material\n-----END PRIVATE KEY-----",
    });

    assert_eq!(
        redact_json(value),
        json!({
            "name": "Production",
            "password": "[REDACTED]",
            "nested": {
                "authorization": "[REDACTED]",
                "database_url": "[REDACTED]",
                "public_key": "ssh-ed25519 AAAA-public",
                "passphrase": "[REDACTED]",
                "api_key": "[REDACTED]",
                "access_key": "[REDACTED]",
                "client_secret": "[REDACTED]",
                "signing_key": "[REDACTED]",
            },
            "repository_url": "https://example.com/team/repository.git",
            "remote": "[REDACTED]",
            "callback_url": "[REDACTED]",
            "private_key_pem": "[REDACTED]",
        })
    );
}

#[test]
fn request_audit_records_complete_platform_context() {
    let actor_user_id = Uuid::now_v7();
    let request_id = Uuid::now_v7();
    let occurred_at = OffsetDateTime::UNIX_EPOCH;

    let event = prepare_request_event(CreateRequestAuditEventParams {
        request_id,
        actor_user_id: Some(actor_user_id),
        actor_node_id: None,
        team_id: None,
        action: "projects.update".to_owned(),
        target_type: "project".to_owned(),
        target_id: Some(Uuid::now_v7()),
        result: AuditEventResult::Success,
        reason: None,
        source_ip: Some("192.0.2.10".to_owned()),
        user_agent: Some("Grass Console".to_owned()),
        http_method: "PATCH".to_owned(),
        request_path: "/api/v1/projects/0195".to_owned(),
        status_code: 200,
        duration_ms: 17,
        changes: json!({
            "before": { "token": "old" },
            "after": { "token": "new" },
        }),
        metadata: json!({ "request_kind": "user", "password": "hidden" }),
        occurred_at,
    });

    assert_eq!(event.request_id, Some(request_id));
    assert_eq!(event.actor_user_id, Some(actor_user_id));
    assert_eq!(event.actor_type, AuditActorType::User);
    assert_eq!(event.visibility, AuditEventVisibility::Platform);
    assert_eq!(event.status_code, Some(200));
    assert_eq!(event.duration_ms, Some(17));
    assert_eq!(event.created_at, occurred_at);
    assert_eq!(event.metadata["password"], "[REDACTED]");
    assert_eq!(event.changes["before"]["token"], "[REDACTED]");
    assert_eq!(event.changes["after"]["token"], "[REDACTED]");
}

#[test]
fn request_audit_identifies_authenticated_node_actor() {
    let node_id = Uuid::now_v7();

    let event = prepare_request_event(CreateRequestAuditEventParams {
        request_id: Uuid::now_v7(),
        actor_user_id: None,
        actor_node_id: Some(node_id),
        team_id: None,
        action: "api.request.post /api/v1/internal/deployments/claim".to_owned(),
        target_type: "deployment".to_owned(),
        target_id: None,
        result: AuditEventResult::Success,
        reason: None,
        source_ip: Some("192.0.2.20".to_owned()),
        user_agent: Some("grass-node/0.1.0".to_owned()),
        http_method: "POST".to_owned(),
        request_path: "/api/v1/internal/deployments/claim".to_owned(),
        status_code: 200,
        duration_ms: 5,
        changes: json!({}),
        metadata: json!({}),
        occurred_at: OffsetDateTime::UNIX_EPOCH,
    });

    assert_eq!(event.actor_type, AuditActorType::Node);
    assert_eq!(event.actor_node_id, Some(node_id));
    assert_eq!(event.actor_user_id, None);
}

#[test]
fn request_audit_prefers_node_actor_over_ambient_user_session() {
    let user_id = Uuid::now_v7();
    let node_id = Uuid::now_v7();

    let event = prepare_request_event(CreateRequestAuditEventParams {
        request_id: Uuid::now_v7(),
        actor_user_id: Some(user_id),
        actor_node_id: Some(node_id),
        team_id: None,
        action: "api.request.post /api/v1/internal/deployments/claim".to_owned(),
        target_type: "deployment".to_owned(),
        target_id: None,
        result: AuditEventResult::Success,
        reason: None,
        source_ip: None,
        user_agent: None,
        http_method: "POST".to_owned(),
        request_path: "/api/v1/internal/deployments/claim".to_owned(),
        status_code: 200,
        duration_ms: 1,
        changes: json!({}),
        metadata: json!({}),
        occurred_at: OffsetDateTime::UNIX_EPOCH,
    });

    assert_eq!(event.actor_type, AuditActorType::Node);
    assert_eq!(event.actor_node_id, Some(node_id));
    assert_eq!(event.actor_user_id, None);
}

#[test]
fn domain_audit_classifies_user_node_and_system_actors() {
    let user_id = Uuid::now_v7();
    let node_id = Uuid::now_v7();

    assert_eq!(domain_actor_type(Some(user_id), None), AuditActorType::User);
    assert_eq!(domain_actor_type(None, Some(node_id)), AuditActorType::Node);
    assert_eq!(domain_actor_type(None, None), AuditActorType::System);
}

#[tokio::test]
async fn retention_prunes_only_events_before_the_cutoff() {
    let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
        .append_exec_results([sea_orm::MockExecResult {
            last_insert_id: 0,
            rows_affected: 3,
        }])
        .into_connection();
    let cutoff = OffsetDateTime::UNIX_EPOCH + time::Duration::days(90);

    let deleted = prune_events_before(&db, cutoff).await.unwrap();

    assert_eq!(deleted, 3);
    let statements = format!("{:?}", db.into_transaction_log());
    assert!(statements.contains("DELETE FROM \\\"audit_events\\\""));
    assert!(statements.contains("\\\"created_at\\\" <"));
}

#[tokio::test]
async fn cleanup_deletes_with_one_filtered_statement_without_materializing_ids() {
    let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
        .append_exec_results([sea_orm::MockExecResult {
            last_insert_id: 0,
            rows_affected: 42,
        }])
        .into_connection();

    let deleted = delete_events(
        &db,
        AuditEventFilter {
            action: Some("auth.".to_owned()),
            actor_type: Some(AuditActorType::User),
            created_to: Some(OffsetDateTime::UNIX_EPOCH),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(deleted, 42);
    let statements = format!("{:?}", db.into_transaction_log());
    assert_eq!(statements.matches("Statement").count(), 1, "{statements}");
    assert!(
        statements.contains("DELETE FROM \\\"audit_events\\\""),
        "{statements}"
    );
    assert!(statements.contains("\\\"action\\\" LIKE"), "{statements}");
    assert!(statements.contains("\\\"actor_type\\\" ="), "{statements}");
    assert!(statements.contains("\\\"created_at\\\" <="), "{statements}");
    assert!(!statements.contains("SELECT"), "{statements}");
    assert!(!statements.contains(" IN "), "{statements}");
}

#[tokio::test]
async fn request_audit_insert_does_not_require_a_returning_row() {
    let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
        .append_exec_results([sea_orm::MockExecResult {
            last_insert_id: 0,
            rows_affected: 1,
        }])
        .into_connection();

    let result = create_request_audit_event(
        &db,
        CreateRequestAuditEventParams {
            request_id: Uuid::now_v7(),
            actor_user_id: None,
            actor_node_id: None,
            team_id: None,
            action: "auth.login.invalid_credentials".to_owned(),
            target_type: "authentication".to_owned(),
            target_id: None,
            result: AuditEventResult::Denied,
            reason: Some("invalid email or password".to_owned()),
            source_ip: Some("192.0.2.10".to_owned()),
            user_agent: None,
            http_method: "POST".to_owned(),
            request_path: "/api/v1/auth/login".to_owned(),
            status_code: 401,
            duration_ms: 2,
            changes: json!({}),
            metadata: json!({}),
            occurred_at: OffsetDateTime::UNIX_EPOCH,
        },
    )
    .await;

    assert!(result.is_ok());
}

#[test]
fn platform_visibility_overrides_team_scope_for_advanced_events() {
    let team_id = Uuid::now_v7();

    assert_eq!(
        visibility_for_domain_event(Some(team_id), None),
        AuditEventVisibility::Team
    );
    assert_eq!(
        visibility_for_domain_event(Some(team_id), Some(AuditEventVisibility::Platform)),
        AuditEventVisibility::Platform
    );
}

#[tokio::test]
async fn platform_change_audit_persists_redacted_before_and_after_values() {
    let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
        .append_exec_results([sea_orm::MockExecResult {
            last_insert_id: 0,
            rows_affected: 1,
        }])
        .into_connection();

    create_platform_audit_event_with_changes(
        &db,
        CreateAuditEventParams {
            actor_user_id: Some(Uuid::now_v7()),
            actor_node_id: None,
            team_id: None,
            action: "settings.updated".to_owned(),
            target_type: "settings".to_owned(),
            target_id: None,
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "changed": ["site_name", "token"] }),
        },
        json!({
            "before": { "site_name": "Old", "token": "old-token" },
            "after": { "site_name": "New", "token": "new-token" },
        }),
    )
    .await
    .unwrap();

    let statements = format!("{:?}", db.into_transaction_log());
    assert!(statements.contains("Old"));
    assert!(statements.contains("New"));
    assert!(!statements.contains("old-token"));
    assert!(!statements.contains("new-token"));
    assert!(statements.contains("[REDACTED]"));
}

#[tokio::test]
async fn domain_audit_identifies_node_actor() {
    let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
        .append_exec_results([sea_orm::MockExecResult {
            last_insert_id: 0,
            rows_affected: 1,
        }])
        .into_connection();
    let node_id = Uuid::now_v7();

    create_audit_event(
        &db,
        CreateAuditEventParams {
            actor_user_id: None,
            actor_node_id: Some(node_id),
            team_id: None,
            action: "node.registered".to_owned(),
            target_type: "node".to_owned(),
            target_id: Some(node_id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({}),
        },
    )
    .await
    .unwrap();

    let statements = format!("{:?}", db.into_transaction_log());
    assert!(statements.contains(&node_id.to_string()), "{statements}");
    assert!(statements.contains("node"), "{statements}");
}

#[tokio::test]
async fn team_audit_query_requires_team_visibility() {
    let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
        .append_query_results([
            Vec::<crate::infra::database::entity::audit_event::Model>::new(),
            Vec::<crate::infra::database::entity::audit_event::Model>::new(),
        ])
        .into_connection();

    list_events(
        &db,
        AuditEventFilter {
            action: None,
            target_id: None,
            team_id: Some(Uuid::now_v7()),
            visibility: Some(AuditEventVisibility::Team),
            team_visible_only: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let statements = format!("{:?}", db.into_transaction_log());
    assert!(
        statements.contains("visibility") && statements.contains("team"),
        "{statements}"
    );
}

#[derive(Clone, Default)]
struct CapturedAuditLog(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for CapturedAuditLog {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl CapturedAuditLog {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

fn transaction_event() -> CreateAuditEventParams {
    CreateAuditEventParams {
        actor_user_id: Some(Uuid::nil()),
        actor_node_id: None,
        team_id: Some(Uuid::nil()),
        action: "project.updated".to_owned(),
        target_type: "project".to_owned(),
        target_id: Some(Uuid::nil()),
        result: AuditEventResult::Success,
        reason: None,
        metadata: serde_json::json!({"password": "must-not-appear-in-audit-log"}),
    }
}

#[tokio::test]
async fn business_audit_logs_wait_for_commit_and_share_request_context() {
    use tracing::instrument::WithSubscriber;

    let capture = CapturedAuditLog::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let request_id = Uuid::now_v7();
    super::context::scope(request_id, async {
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_exec_results([sea_orm::MockExecResult {
                rows_affected: 1,
                last_insert_id: 0,
            }])
            .into_connection();
        let transaction = super::AuditTransaction::begin(&db).await.unwrap();
        create_audit_event(&transaction, transaction_event())
            .await
            .unwrap();
        assert!(
            capture.text().is_empty(),
            "an uncommitted insert must not log success"
        );
        transaction.commit().await.unwrap();
        let output = capture.text();
        assert_eq!(output.matches("audit event committed").count(), 1);
        assert!(output.contains(&request_id.to_string()));
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("must-not-appear-in-audit-log"));
    })
    .with_subscriber(subscriber)
    .await;
}

#[tokio::test]
async fn rolled_back_audit_never_logs_committed_success() {
    use tracing::instrument::WithSubscriber;

    let capture = CapturedAuditLog::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    async {
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_exec_results([sea_orm::MockExecResult {
                rows_affected: 1,
                last_insert_id: 0,
            }])
            .into_connection();
        let transaction = super::AuditTransaction::begin(&db).await.unwrap();
        create_audit_event(&transaction, transaction_event())
            .await
            .unwrap();
        transaction.rollback().await.unwrap();
        assert!(capture.text().is_empty());
        let log = format!("{:?}", db.into_transaction_log());
        assert!(log.contains("ROLLBACK"));
        assert!(!log.contains("COMMIT"));
    }
    .with_subscriber(subscriber)
    .await;
}

#[tokio::test]
async fn audit_insert_failure_is_returned_and_does_not_log_success() {
    use tracing::instrument::WithSubscriber;

    let capture = CapturedAuditLog::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    async {
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_exec_errors([sea_orm::DbErr::Custom("audit unavailable".to_owned())])
            .into_connection();
        let transaction = super::AuditTransaction::begin(&db).await.unwrap();
        assert!(
            create_audit_event(&transaction, transaction_event())
                .await
                .is_err()
        );
        drop(transaction);
        let output = capture.text();
        assert!(output.contains("audit event persistence failed"));
        assert!(!output.contains("audit event committed"));
    }
    .with_subscriber(subscriber)
    .await;
}

#[tokio::test]
async fn free_text_reasons_are_redacted_before_persistence() {
    let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
        .append_exec_results([sea_orm::MockExecResult {
            rows_affected: 1,
            last_insert_id: 0,
        }])
        .into_connection();
    let mut event = transaction_event();
    event.reason = Some("upstream rejected Bearer must-not-appear".to_owned());
    create_audit_event(&db, event).await.unwrap();
    let statements = format!("{:?}", db.into_transaction_log());
    assert!(!statements.contains("must-not-appear"));
    assert!(statements.contains("[REDACTED]"));
}

#[test]
fn free_text_redaction_keeps_safe_diagnostics() {
    for secret in [
        "upstream returned bearer credential-value",
        "connection failed: postgres://user:password@example.invalid/db",
        "request rejected (https://example.invalid/?token=secret)",
        "authentication failed: password=hidden",
    ] {
        assert_eq!(
            super::redaction::redact_text(secret.to_owned()),
            "[REDACTED]"
        );
    }
    let safe = "connection timed out after 30 seconds";
    assert_eq!(super::redaction::redact_text(safe.to_owned()), safe);
}
