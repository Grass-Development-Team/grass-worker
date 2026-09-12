use crate::infra::database::entity::audit_event;

/// Called only after an autocommit insertion or its owning transaction commits.
pub(super) fn committed(event: &audit_event::Model) {
    tracing::info!(
        target: "control_api::audit",
        audit_id = %event.id,
        request_id = ?event.request_id,
        actor_type = ?event.actor_type,
        actor_user_id = ?event.actor_user_id,
        actor_node_id = ?event.actor_node_id,
        team_id = ?event.team_id,
        visibility = ?event.visibility,
        action = %event.action,
        target_type = %event.target_type,
        target_id = ?event.target_id,
        result = ?event.result,
        reason = ?event.reason,
        changes = %event.changes,
        metadata = %event.metadata,
        status_code = ?event.status_code,
        duration_ms = ?event.duration_ms,
        "audit event committed"
    );
}

pub(super) fn persistence_failed(event: &audit_event::Model, error: &sea_orm::DbErr) {
    let diagnostic = super::redaction::redact_text(error.to_string());
    tracing::error!(
        target: "control_api::audit",
        audit_id = %event.id,
        request_id = ?event.request_id,
        action = %event.action,
        error = %diagnostic,
        "audit event persistence failed"
    );
}
