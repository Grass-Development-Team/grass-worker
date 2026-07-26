use axum::{Json, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::{
        quotas::QuotaDimension,
        teams::{self, AcceptInvitationParams, CreateInvitationParams, InvitationError},
    },
    infra::{
        error::{AppError, ok_response},
        http::extractors::{Session, TeamRole},
        quota::{QuotaCharge, QuotaService},
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct CreateInvitationRequest {
    pub email: String,
    pub role: String,
}

#[derive(Deserialize)]
pub struct AcceptInvitationRequest {
    pub token: String,
}

pub async fn create(
    axum::extract::State(state): axum::extract::State<ControlApiState>,
    team_role: TeamRole,
    Json(body): Json<CreateInvitationRequest>,
) -> Result<impl IntoResponse, AppError> {
    team_role.require_admin("teams.invitations.create.admin_required")?;

    let email =
        grass_validator::normalize_email(&body.email).map_err(|error| AppError::Validation {
            op: "teams.invitations.create.invalid_email",
            message: error.to_string(),
        })?;

    let role = super::parse_role(&body.role, "teams.invitations.create.invalid_role")?;
    teams::validate_managed_member_role(&role).map_err(|error| AppError::Validation {
        op: "teams.invitations.create.owner_role",
        message: error.to_string(),
    })?;

    let db = super::database(&state, "teams.invitations.create.no_database")?;
    let cache = super::cache(&state, "teams.invitations.create.no_cache")?;

    // Member quota is consumed when an invitation is accepted; inviting only
    // pre-checks the limit so teams cannot fan out invitations they can never
    // accept.
    let team = teams::get_by_id(db, team_role.team_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.invitations.create.team_lookup",
            source,
        })?
        .ok_or_else(|| AppError::NotFound {
            op: "teams.invitations.create.team_not_found",
            message: "team not found".to_owned(),
        })?;
    let quota = QuotaService::new(db, cache);
    let precheck = quota
        .reserve(
            "teams.invitations.create.quota",
            &team,
            Some(team_role.user_id),
            &[QuotaCharge::one(QuotaDimension::Members)],
        )
        .await?;
    quota.rollback(precheck).await;

    let token = grass_token::generate_token();
    let invitation = teams::create_invitation(
        db,
        CreateInvitationParams {
            team_id: team_role.team_id,
            email,
            role,
            invited_by_user_id: team_role.user_id,
            token_hash: teams::invitation_token_hash(&token),
        },
    )
    .await
    .map_err(map_invitation_error)?;

    Ok(ok_response(json!({
        "invitation": {
            "id": invitation.id,
            "team_id": invitation.team_id,
            "email": invitation.email,
            "role": super::role_value(&invitation.role),
            "status": "pending",
            "expires_at": invitation.expires_at,
            "token": token,
        }
    })))
}

pub async fn accept(
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

    let db = super::database(&state, "teams.invitations.accept.no_database")?;
    let cache = super::cache(&state, "teams.invitations.accept.no_cache")?;
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

    Ok(ok_response(json!({
        "member": {
            "id": member.id,
            "team_id": member.team_id,
            "user_id": member.user_id,
            "role": super::role_value(&member.role),
            "joined_at": member.joined_at,
        }
    })))
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
        InvitationError::Database(source) => AppError::Infrastructure {
            op: "teams.invitations.database",
            source: source.into(),
        },
    }
}
