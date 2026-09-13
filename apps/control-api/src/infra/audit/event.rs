use super::redact_json;
use crate::infra::database::entity::{
    AuditActorType, AuditEventResult, AuditEventVisibility, audit_event,
};
use time::OffsetDateTime;
use uuid::Uuid;

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

pub(super) fn domain_actor_type(
    actor_user_id: Option<Uuid>,
    actor_node_id: Option<Uuid>,
) -> AuditActorType {
    match (actor_user_id, actor_node_id) {
        (Some(_), _) => AuditActorType::User,
        (None, Some(_)) => AuditActorType::Node,
        (None, None) => AuditActorType::System,
    }
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
