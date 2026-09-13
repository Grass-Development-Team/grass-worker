use super::event::domain_actor_type;
use super::{
    CreateAuditEventParams, CreateRequestAuditEventParams, prepare_request_event, redact_json,
    visibility_for_domain_event,
};
use crate::infra::database::entity::{AuditEventVisibility, audit_event};
use sea_orm::{ActiveValue::Set, EntityTrait, TryIntoModel};
use time::OffsetDateTime;
use uuid::Uuid;

pub async fn create_request_audit_event<C: super::AuditConnection>(
    db: &C,
    params: CreateRequestAuditEventParams,
) -> anyhow::Result<()> {
    let event = prepare_request_event(params);
    persist(db, event).await
}

pub async fn create_audit_event<C: super::AuditConnection>(
    db: &C,
    params: CreateAuditEventParams,
) -> anyhow::Result<()> {
    create_domain_audit_event(db, params, None, serde_json::json!({})).await
}

pub async fn create_audit_event_with_changes<C: super::AuditConnection>(
    db: &C,
    params: CreateAuditEventParams,
    changes: serde_json::Value,
) -> anyhow::Result<()> {
    create_domain_audit_event(db, params, None, changes).await
}

pub async fn create_platform_audit_event<C: super::AuditConnection>(
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

pub async fn create_audit_event_with_visibility<C: super::AuditConnection>(
    db: &C,
    params: CreateAuditEventParams,
    visibility: AuditEventVisibility,
) -> anyhow::Result<()> {
    create_domain_audit_event(db, params, Some(visibility), serde_json::json!({})).await
}

pub async fn create_platform_audit_event_with_changes<C: super::AuditConnection>(
    db: &C,
    params: CreateAuditEventParams,
    changes: serde_json::Value,
) -> anyhow::Result<()> {
    create_domain_audit_event(db, params, Some(AuditEventVisibility::Platform), changes).await
}

async fn create_domain_audit_event<C: super::AuditConnection>(
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
        request_id: Set(super::context::request_id()),
        source_ip: Set(None),
        user_agent: Set(None),
        http_method: Set(None),
        request_path: Set(None),
        status_code: Set(None),
        duration_ms: Set(None),
        changes: Set(redact_json(changes)),
        created_at: Set(OffsetDateTime::now_utc()),
    };
    persist(db, event.try_into_model()?).await
}

async fn persist<C: super::AuditConnection>(
    db: &C,
    mut event: audit_event::Model,
) -> anyhow::Result<()> {
    event.reason = event.reason.map(super::redaction::redact_text);
    if let Err(error) = audit_event::Entity::insert(audit_event::ActiveModel::from(event.clone()))
        .exec_without_returning(db)
        .await
    {
        super::logging::persistence_failed(&event, &error);
        return Err(error.into());
    }
    db.audit_written(event);
    Ok(())
}

/// Records an external or post-response observation. Persistence failures are
/// logged by the writer; they cannot undo the already completed operation.
pub async fn observe_event(db: &sea_orm::DatabaseConnection, params: CreateAuditEventParams) {
    let _ = create_audit_event(db, params).await;
}

/// Platform-visible counterpart of `observe_event`.
pub async fn observe_platform_event(
    db: &sea_orm::DatabaseConnection,
    params: CreateAuditEventParams,
) {
    let _ = create_platform_audit_event(db, params).await;
}

/// Request completion is observed after the business response is produced.
pub async fn observe_request(
    db: &sea_orm::DatabaseConnection,
    params: CreateRequestAuditEventParams,
) {
    let _ = create_request_audit_event(db, params).await;
}
