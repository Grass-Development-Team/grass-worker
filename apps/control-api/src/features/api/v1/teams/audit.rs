use axum::{
    extract::{Query, State},
    response::IntoResponse,
};
use serde_json::json;

use crate::{
    domain::audits,
    infra::{
        database::entity::AuditEventVisibility,
        error::{AppError, ok_response},
        http::extractors::TeamRole,
    },
    state::ControlApiState,
};

fn authorize_team_audit(role: &TeamRole) -> Result<(), AppError> {
    role.require_admin("teams.audit_events.list.admin_required")
}

/// GET /api/v1/teams/{team_id}/audit-events
pub async fn list(
    State(state): State<ControlApiState>,
    role: TeamRole,
    Query(query): Query<crate::features::api::v1::admin::audit_events::AuditQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "teams.audit_events.list";
    authorize_team_audit(&role)?;
    let db = super::database(&state, OP)?;

    let page = audits::list_events(
        db,
        crate::features::api::v1::admin::audit_events::event_filter(
            query,
            Some(role.team_id),
            Some(AuditEventVisibility::Team),
            true,
            OP,
        )?,
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(json!({
        "events": page.events
            .iter()
            .map(crate::features::api::v1::admin::audit_events::event_view)
            .collect::<Vec<_>>(),
        "pagination": {
            "page": page.page,
            "per_page": page.per_page,
            "total": page.total,
            "total_pages": page.total_pages,
        },
    })))
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::{infra::database::entity::TeamMemberRole, infra::http::extractors::TeamRole};

    use super::authorize_team_audit;

    fn role(role: TeamMemberRole) -> TeamRole {
        TeamRole {
            team_id: Uuid::now_v7(),
            user_id: Uuid::now_v7(),
            role,
        }
    }

    #[test]
    fn team_audit_is_limited_to_owners_and_admins() {
        assert!(authorize_team_audit(&role(TeamMemberRole::Owner)).is_ok());
        assert!(authorize_team_audit(&role(TeamMemberRole::Admin)).is_ok());
        assert!(authorize_team_audit(&role(TeamMemberRole::Member)).is_err());
        assert!(authorize_team_audit(&role(TeamMemberRole::Viewer)).is_err());
    }
}
