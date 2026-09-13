use axum::{Json, response::IntoResponse};
use serde::Deserialize;

use crate::{
    domain::{
        quotas::QuotaDimension,
        teams::{self, AcceptInvitationParams, InvitationError},
    },
    infra::{
        error::{AppError, ok_response},
        http::extractors::Session,
        quota::{QuotaCharge, QuotaService},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/team-invitations/accept", axum::routing::post(accept))
}

fn role_value(role: &crate::infra::database::entity::TeamMemberRole) -> &'static str {
    use crate::infra::database::entity::TeamMemberRole;

    match role {
        TeamMemberRole::Owner => "owner",
        TeamMemberRole::Admin => "admin",
        TeamMemberRole::Member => "member",
        TeamMemberRole::Viewer => "viewer",
    }
}

#[derive(Deserialize)]
struct AcceptInvitationRequest {
    token: String,
}

async fn accept(
    axum::extract::State(state): axum::extract::State<ControlApiState>,
    session: Session,
    Json(body): Json<AcceptInvitationRequest>,
) -> Result<impl IntoResponse, AppError> {
    if body.token.trim().is_empty() {
        return Err(AppError::Validation {
            op: "teams.invitations.accept.invalid_token",
            message: "invitation token is required".to_owned(),
        });
    }

    let db = crate::infra::http::database(&state, "teams.invitations.accept.no_database")?;
    let cache = crate::infra::http::cache(&state, "teams.invitations.accept.no_cache")?;
    let token_hash = teams::invitation_token_hash(body.token.trim());

    let invitation_team_id = teams::invitation_team_by_token_hash(db, &token_hash)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.invitations.accept.lookup",
            source,
        })?
        .ok_or_else(|| AppError::NotFound {
            op: "teams.invitations.not_found",
            message: "invitation not found".to_owned(),
        })?;
    let team = teams::get_by_id(db, invitation_team_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.invitations.accept.team_lookup",
            source,
        })?
        .ok_or_else(|| AppError::NotFound {
            op: "teams.invitations.accept.team_not_found",
            message: "team not found".to_owned(),
        })?;

    let quota = QuotaService::new(db, cache);
    let reservation = quota
        .reserve(
            "teams.invitations.accept.quota",
            &team,
            Some(session.data.user_id),
            &[QuotaCharge::one(QuotaDimension::Members)],
        )
        .await?;

    let member = match teams::accept_invitation(
        db,
        AcceptInvitationParams {
            token_hash,
            user_id: session.data.user_id,
        },
    )
    .await
    {
        Ok(member) => member,
        Err(error) => {
            quota.rollback(reservation).await;
            return Err(map_invitation_error(error));
        }
    };

    quota
        .commit(
            "teams.invitations.accept.quota_commit",
            reservation,
            "team_member",
            Some(member.id),
        )
        .await?;

    Ok(ok_response(AcceptResponse {
        member: AcceptMemberResponse {
            id: member.id,
            team_id: member.team_id,
            user_id: member.user_id,
            role: role_value(&member.role),
            joined_at: member.joined_at,
        },
    }))
}

fn map_invitation_error(error: InvitationError) -> AppError {
    match error {
        InvitationError::NotFound => AppError::NotFound {
            op: "teams.invitations.not_found",
            message: error.to_string(),
        },
        InvitationError::NotPending => AppError::Conflict {
            op: "teams.invitations.not_pending",
            message: error.to_string(),
        },
        InvitationError::Expired => AppError::Gone {
            op: "teams.invitations.expired",
            message: error.to_string(),
        },
        InvitationError::EmailMismatch => AppError::Forbidden {
            op: "teams.invitations.email_mismatch",
            message: error.to_string(),
        },
        InvitationError::OwnerRole => AppError::Validation {
            op: "teams.invitations.owner_role",
            message: error.to_string(),
        },
        InvitationError::AlreadyMember => AppError::Conflict {
            op: "teams.invitations.already_member",
            message: error.to_string(),
        },
        InvitationError::UserNotFound => AppError::NotFound {
            op: "teams.invitations.user_not_found",
            message: error.to_string(),
        },
        InvitationError::Database(source) => AppError::Infrastructure {
            op: "teams.invitations.database",
            source: source.into(),
        },
    }
}

#[derive(serde::Serialize)]
struct AcceptMemberResponse {
    id: uuid::Uuid,
    team_id: uuid::Uuid,
    user_id: uuid::Uuid,
    role: &'static str,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    joined_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct AcceptResponse {
    member: AcceptMemberResponse,
}
