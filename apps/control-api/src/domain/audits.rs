//! Audit event writing shared by every feature that records key behavior.

use sea_orm::{
    ActiveValue::Set, ColumnTrait, Condition, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{
    AuditActorType, AuditEventResult, AuditEventVisibility, audit_event,
};

const REDACTED: &str = "[REDACTED]";

pub fn redact_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(values) => serde_json::Value::Object(
            values
                .into_iter()
                .map(|(key, value)| {
                    let value = if sensitive_key(&key) {
                        serde_json::Value::String(REDACTED.to_owned())
                    } else {
                        redact_json(value)
                    };
                    (key, value)
                })
                .collect(),
        ),
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(redact_json).collect())
        }
        serde_json::Value::String(value) if sensitive_string(&value) => {
            serde_json::Value::String(REDACTED.to_owned())
        }
        other => other,
    }
}

fn sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    matches!(
        normalized.as_str(),
        "authorization"
            | "cookie"
            | "set_cookie"
            | "password"
            | "password_hash"
            | "passphrase"
            | "secret"
            | "secret_key"
            | "secret_ref"
            | "api_key"
            | "access_key"
            | "client_secret"
            | "signing_key"
            | "token"
            | "token_hash"
            | "access_token"
            | "refresh_token"
            | "csrf_token"
            | "session_id"
            | "private_key"
            | "master_key"
            | "credential"
            | "credentials"
            | "git_credentials"
            | "database_url"
            | "redis_url"
            | "connection_string"
    ) || normalized.ends_with("_password")
        || normalized.ends_with("_passphrase")
        || normalized.ends_with("_secret")
        || normalized.ends_with("_api_key")
        || normalized.ends_with("_access_key")
        || normalized.ends_with("_signing_key")
        || normalized.ends_with("_token")
        || normalized.ends_with("_cookie")
        || normalized.ends_with("_private_key")
        || normalized.ends_with("_credential")
        || normalized.ends_with("_credentials")
        || normalized.ends_with("_connection_string")
}

fn sensitive_string(value: &str) -> bool {
    if value
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer "))
    {
        return true;
    }

    let uppercase = value.to_ascii_uppercase();
    if uppercase.contains("-----BEGIN ") && uppercase.contains("PRIVATE KEY-----") {
        return true;
    }

    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    if url
        .query_pairs()
        .any(|(key, _)| sensitive_key(key.as_ref()))
    {
        return true;
    }
    match url.scheme() {
        "http" | "https" => !url.username().is_empty() || url.password().is_some(),
        "postgres" | "postgresql" | "redis" | "rediss" | "mysql" => true,
        _ => url.password().is_some(),
    }
}

pub struct CreateAuditEventParams {
    pub actor_user_id: Option<Uuid>,
    pub actor_node_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<Uuid>,
    pub result: AuditEventResult,
    pub reason: Option<String>,
    pub metadata: serde_json::Value,
}

pub struct CreateRequestAuditEventParams {
    pub request_id: Uuid,
    pub actor_user_id: Option<Uuid>,
    pub actor_node_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<Uuid>,
    pub result: AuditEventResult,
    pub reason: Option<String>,
    pub source_ip: Option<String>,
    pub user_agent: Option<String>,
    pub http_method: String,
    pub request_path: String,
    pub status_code: u16,
    pub duration_ms: u64,
    pub changes: serde_json::Value,
    pub metadata: serde_json::Value,
    pub occurred_at: OffsetDateTime,
}

pub fn prepare_request_event(params: CreateRequestAuditEventParams) -> audit_event::Model {
    let (actor_type, actor_user_id, actor_node_id) = if let Some(node_id) = params.actor_node_id {
        (AuditActorType::Node, None, Some(node_id))
    } else if let Some(user_id) = params.actor_user_id {
        (AuditActorType::User, Some(user_id), None)
    } else {
        (AuditActorType::Anonymous, None, None)
    };

    audit_event::Model {
        id: Uuid::now_v7(),
        actor_user_id,
        actor_node_id,
        team_id: params.team_id,
        actor_type,
        visibility: AuditEventVisibility::Platform,
        action: params.action,
        target_type: params.target_type,
        target_id: params.target_id,
        result: params.result,
        reason: params.reason,
        metadata: redact_json(params.metadata),
        request_id: Some(params.request_id),
        source_ip: params.source_ip,
        user_agent: params.user_agent,
        http_method: Some(params.http_method),
        request_path: Some(params.request_path),
        status_code: Some(i32::from(params.status_code)),
        duration_ms: Some(params.duration_ms.min(i64::MAX as u64) as i64),
        changes: redact_json(params.changes),
        created_at: params.occurred_at,
    }
}

pub async fn create_request_audit_event<C: ConnectionTrait>(
    db: &C,
    params: CreateRequestAuditEventParams,
) -> anyhow::Result<()> {
    let event = prepare_request_event(params);
    audit_event::Entity::insert(audit_event::ActiveModel::from(event))
        .exec_without_returning(db)
        .await
        .map(|_| ())
        .map_err(Into::into)
}

pub async fn create_audit_event<C: ConnectionTrait>(
    db: &C,
    params: CreateAuditEventParams,
) -> anyhow::Result<()> {
    create_domain_audit_event(db, params, None, serde_json::json!({})).await
}

pub async fn create_platform_audit_event<C: ConnectionTrait>(
    db: &C,
    params: CreateAuditEventParams,
) -> anyhow::Result<()> {
    create_domain_audit_event(
        db,
        params,
        Some(AuditEventVisibility::Platform),
        serde_json::json!({}),
    )
    .await
}

pub async fn create_audit_event_with_visibility<C: ConnectionTrait>(
    db: &C,
    params: CreateAuditEventParams,
    visibility: AuditEventVisibility,
) -> anyhow::Result<()> {
    create_domain_audit_event(db, params, Some(visibility), serde_json::json!({})).await
}

pub async fn create_platform_audit_event_with_changes<C: ConnectionTrait>(
    db: &C,
    params: CreateAuditEventParams,
    changes: serde_json::Value,
) -> anyhow::Result<()> {
    create_domain_audit_event(db, params, Some(AuditEventVisibility::Platform), changes).await
}

fn domain_actor_type(actor_user_id: Option<Uuid>, actor_node_id: Option<Uuid>) -> AuditActorType {
    match (actor_user_id, actor_node_id) {
        (Some(_), _) => AuditActorType::User,
        (None, Some(_)) => AuditActorType::Node,
        (None, None) => AuditActorType::System,
    }
}

async fn create_domain_audit_event<C: ConnectionTrait>(
    db: &C,
    params: CreateAuditEventParams,
    visibility: Option<AuditEventVisibility>,
    changes: serde_json::Value,
) -> anyhow::Result<()> {
    let visibility = visibility_for_domain_event(params.team_id, visibility);
    let event = audit_event::ActiveModel {
        id: Set(Uuid::now_v7()),
        actor_user_id: Set(params.actor_user_id),
        actor_node_id: Set(params.actor_node_id),
        team_id: Set(params.team_id),
        actor_type: Set(domain_actor_type(
            params.actor_user_id,
            params.actor_node_id,
        )),
        visibility: Set(visibility),
        action: Set(params.action),
        target_type: Set(params.target_type),
        target_id: Set(params.target_id),
        result: Set(params.result),
        reason: Set(params.reason),
        metadata: Set(redact_json(params.metadata)),
        request_id: Set(None),
        source_ip: Set(None),
        user_agent: Set(None),
        http_method: Set(None),
        request_path: Set(None),
        status_code: Set(None),
        duration_ms: Set(None),
        changes: Set(redact_json(changes)),
        created_at: Set(OffsetDateTime::now_utc()),
    };
    audit_event::Entity::insert(event)
        .exec_without_returning(db)
        .await
        .map(|_| ())
        .map_err(Into::into)
}

pub fn visibility_for_domain_event(
    team_id: Option<Uuid>,
    override_visibility: Option<AuditEventVisibility>,
) -> AuditEventVisibility {
    override_visibility.unwrap_or(if team_id.is_some() {
        AuditEventVisibility::Team
    } else {
        AuditEventVisibility::Platform
    })
}

#[derive(Default)]
pub struct AuditEventFilter {
    pub action: Option<String>,
    pub actor_user_id: Option<Uuid>,
    pub actor_type: Option<AuditActorType>,
    pub target_type: Option<String>,
    pub target_id: Option<Uuid>,
    /// Restrict to one team's events; `None` is the platform-wide view.
    pub team_id: Option<Uuid>,
    pub result: Option<AuditEventResult>,
    pub created_from: Option<OffsetDateTime>,
    pub created_to: Option<OffsetDateTime>,
    pub visibility: Option<AuditEventVisibility>,
    /// Team audit is a curated business view, not a filtered platform log.
    pub team_visible_only: bool,
    pub page: u64,
    pub per_page: u64,
}

pub fn audit_event_query(filter: &AuditEventFilter) -> sea_orm::Select<audit_event::Entity> {
    let mut query = audit_event::Entity::find();
    if let Some(action) = filter.action.as_deref() {
        query = query.filter(audit_event::Column::Action.starts_with(action));
    }
    if let Some(actor_user_id) = filter.actor_user_id {
        query = query.filter(audit_event::Column::ActorUserId.eq(actor_user_id));
    }
    if let Some(actor_type) = filter.actor_type.as_ref() {
        query = query.filter(audit_event::Column::ActorType.eq(actor_type.clone()));
    }
    if let Some(target_type) = filter.target_type.as_deref() {
        query = query.filter(audit_event::Column::TargetType.eq(target_type));
    }
    if let Some(target_id) = filter.target_id {
        query = query.filter(audit_event::Column::TargetId.eq(target_id));
    }
    if let Some(team_id) = filter.team_id {
        query = query.filter(audit_event::Column::TeamId.eq(team_id));
    }
    if let Some(visibility) = filter.visibility.as_ref() {
        query = query.filter(audit_event::Column::Visibility.eq(visibility.clone()));
    }
    if let Some(result) = filter.result.as_ref() {
        query = query.filter(audit_event::Column::Result.eq(result.clone()));
    }
    if let Some(created_from) = filter.created_from {
        query = query.filter(audit_event::Column::CreatedAt.gte(created_from));
    }
    if let Some(created_to) = filter.created_to {
        query = query.filter(audit_event::Column::CreatedAt.lte(created_to));
    }
    if filter.team_visible_only {
        query = query.filter(
            Condition::any()
                .add(audit_event::Column::Action.starts_with("deployment."))
                .add(audit_event::Column::Action.eq("artifact.uploaded"))
                .add(audit_event::Column::Action.starts_with("host."))
                .add(audit_event::Column::Action.starts_with("project."))
                .add(audit_event::Column::Action.starts_with("quota."))
                .add(audit_event::Column::Action.starts_with("team.member."))
                .add(audit_event::Column::Action.starts_with("team.invitation.")),
        );
    }

    query
}

pub struct AuditEventPage {
    pub events: Vec<audit_event::Model>,
    pub page: u64,
    pub per_page: u64,
    pub total: u64,
    pub total_pages: u64,
}

pub async fn list_events<C: ConnectionTrait>(
    db: &C,
    filter: AuditEventFilter,
) -> anyhow::Result<AuditEventPage> {
    use sea_orm::{QueryOrder, QuerySelect};

    let page = filter.page.max(1);
    let per_page = match filter.per_page {
        0 => 50,
        value => value.clamp(1, 100),
    };
    let query = audit_event_query(&filter);
    let total = query.clone().count(db).await?;
    let events = query
        .order_by_desc(audit_event::Column::CreatedAt)
        .order_by_desc(audit_event::Column::Id)
        .offset((page - 1).saturating_mul(per_page))
        .limit(per_page)
        .all(db)
        .await?;

    Ok(AuditEventPage {
        events,
        page,
        per_page,
        total,
        total_pages: total.div_ceil(per_page),
    })
}

pub async fn prune_events_before<C: ConnectionTrait>(
    db: &C,
    cutoff: OffsetDateTime,
) -> anyhow::Result<u64> {
    audit_event::Entity::delete_many()
        .filter(audit_event::Column::CreatedAt.lt(cutoff))
        .exec(db)
        .await
        .map(|result| result.rows_affected)
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use time::OffsetDateTime;
    use uuid::Uuid;

    use crate::infra::database::entity::{AuditActorType, AuditEventResult, AuditEventVisibility};

    use super::{
        AuditEventFilter, CreateAuditEventParams, CreateRequestAuditEventParams, audit_event_query,
        create_audit_event, create_platform_audit_event_with_changes, create_request_audit_event,
        domain_actor_type, list_events, prepare_request_event, prune_events_before, redact_json,
        visibility_for_domain_event,
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
}
