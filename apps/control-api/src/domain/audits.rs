//! Audit event writing shared by every feature that records key behavior.

use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, QueryFilter};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{AuditEventResult, audit_event};

pub struct CreateAuditEventParams {
    pub actor_user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<Uuid>,
    pub result: AuditEventResult,
    pub reason: Option<String>,
    pub metadata: serde_json::Value,
}

pub async fn create_audit_event<C: ConnectionTrait>(
    db: &C,
    params: CreateAuditEventParams,
) -> anyhow::Result<audit_event::Model> {
    audit_event::ActiveModel {
        id: Set(Uuid::now_v7()),
        actor_user_id: Set(params.actor_user_id),
        team_id: Set(params.team_id),
        action: Set(params.action),
        target_type: Set(params.target_type),
        target_id: Set(params.target_id),
        result: Set(params.result),
        reason: Set(params.reason),
        metadata: Set(params.metadata),
        created_at: Set(OffsetDateTime::now_utc()),
    }
    .insert(db)
    .await
    .map_err(Into::into)
}

pub struct AuditEventFilter {
    pub action: Option<String>,
    pub target_id: Option<Uuid>,
    /// Restrict to one team's events; `None` is the platform-wide view.
    pub team_id: Option<Uuid>,
    pub limit: u64,
}

pub async fn list_events<C: ConnectionTrait>(
    db: &C,
    filter: AuditEventFilter,
) -> anyhow::Result<Vec<audit_event::Model>> {
    use sea_orm::{EntityTrait, QueryOrder, QuerySelect};

    let mut query = audit_event::Entity::find();
    if let Some(action) = filter.action {
        query = query.filter(audit_event::Column::Action.starts_with(&action));
    }
    if let Some(target_id) = filter.target_id {
        query = query.filter(audit_event::Column::TargetId.eq(target_id));
    }
    if let Some(team_id) = filter.team_id {
        query = query.filter(audit_event::Column::TeamId.eq(team_id));
    }

    query
        .order_by_desc(audit_event::Column::CreatedAt)
        .limit(filter.limit.clamp(1, 500))
        .all(db)
        .await
        .map_err(Into::into)
}
