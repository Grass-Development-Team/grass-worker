use axum::{extract::Query, response::IntoResponse};
use serde::Deserialize;

use crate::{
    domain::{teams, users},
    infra::{
        error::{AppError, ok_response},
        http::extractors::session::OptionalSession,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/team-invitations/preflight", axum::routing::get(preflight))
}

pub(crate) fn role_value(role: &crate::infra::database::entity::TeamMemberRole) -> &'static str {
    use crate::infra::database::entity::TeamMemberRole;

    match role {
        TeamMemberRole::Owner => "owner",
        TeamMemberRole::Admin => "admin",
        TeamMemberRole::Member => "member",
        TeamMemberRole::Viewer => "viewer",
    }
}

#[derive(Deserialize)]
pub struct PreflightInvitationQuery {
    pub token: String,
}

pub async fn preflight(
    axum::extract::State(state): axum::extract::State<ControlApiState>,
    optional_session: OptionalSession,
    Query(query): Query<PreflightInvitationQuery>,
) -> Result<impl IntoResponse, AppError> {
    let token = query.token.trim();
    if token.is_empty() {
        return Err(AppError::Validation {
            op: "teams.invitations.preflight.invalid_token",
            message: "invitation token is required".to_owned(),
        });
    }

    let db = crate::infra::http::database(&state, "teams.invitations.preflight.no_database")?;
    let invitation = teams::invitation_by_token_hash(db, &teams::invitation_token_hash(token))
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.invitations.preflight.lookup",
            source,
        })?
        .ok_or_else(|| AppError::NotFound {
            op: "teams.invitations.not_found",
            message: "invitation not found".to_owned(),
        })?;
    let team = teams::get_by_id(db, invitation.team_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.invitations.preflight.team_lookup",
            source,
        })?
        .ok_or_else(|| AppError::NotFound {
            op: "teams.invitations.preflight.team_not_found",
            message: "team not found".to_owned(),
        })?;

    let email_matches_current_user = if let Some(session) = optional_session.0 {
        let current_user = users::get_user_by_id(db, session.data.user_id)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: "teams.invitations.preflight.user_lookup",
                source,
            })?
            .ok_or_else(|| AppError::NotFound {
                op: "teams.invitations.preflight.user_not_found",
                message: "user not found".to_owned(),
            })?;
        Some(invitation.email.eq_ignore_ascii_case(&current_user.email))
    } else {
        None
    };

    let status = if !matches!(
        invitation.status,
        crate::infra::database::entity::TeamInvitationStatus::Pending
    ) {
        invitation_status_value(&invitation.status)
    } else if invitation.expires_at <= time::OffsetDateTime::now_utc() {
        "expired"
    } else if email_matches_current_user == Some(false) {
        "email_mismatch"
    } else {
        "pending"
    };
    let can_accept = status == "pending" && email_matches_current_user == Some(true);

    Ok(ok_response(PreflightResponse {
        team: PreflightTeamResponse {
            id: team.id,
            name: team.name.clone(),
        },
        role: role_value(&invitation.role),
        expires_at: invitation.expires_at,
        status,
        email_matches_current_user,
        can_accept,
    }))
}

fn invitation_status_value(
    status: &crate::infra::database::entity::TeamInvitationStatus,
) -> &'static str {
    use crate::infra::database::entity::TeamInvitationStatus;

    match status {
        TeamInvitationStatus::Pending => "pending",
        TeamInvitationStatus::Accepted => "accepted",
        TeamInvitationStatus::Expired => "expired",
        TeamInvitationStatus::Revoked => "revoked",
    }
}

#[derive(serde::Serialize)]
struct PreflightTeamResponse {
    id: uuid::Uuid,
    name: String,
}

#[derive(serde::Serialize)]
struct PreflightResponse {
    team: PreflightTeamResponse,
    role: &'static str,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    expires_at: time::OffsetDateTime,
    status: &'static str,
    email_matches_current_user: Option<bool>,
    can_accept: bool,
}
