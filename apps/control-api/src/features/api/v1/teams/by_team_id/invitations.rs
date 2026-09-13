use axum::{Json, response::IntoResponse};
use serde::Deserialize;

use crate::{
    domain::{
        platform_mail,
        quotas::QuotaDimension,
        registration::SignupPolicy,
        settings,
        teams::{self, CreateInvitationParams, InvitationError},
    },
    infra::{
        error::{AppError, ok_response},
        http::extractors::TeamRole,
        quota::{QuotaCharge, QuotaService},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/teams/{team_id}/invitations", axum::routing::post(create))
}

fn parse_role(
    role: &str,
    op: &'static str,
) -> Result<crate::infra::database::entity::TeamMemberRole, AppError> {
    use crate::infra::database::entity::TeamMemberRole;

    match role.trim().to_lowercase().as_str() {
        "owner" => Ok(TeamMemberRole::Owner),
        "admin" => Ok(TeamMemberRole::Admin),
        "member" => Ok(TeamMemberRole::Member),
        "viewer" => Ok(TeamMemberRole::Viewer),
        _ => Err(AppError::Validation {
            op,
            message: format!("invalid role: {role}"),
        }),
    }
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
struct CreateInvitationRequest {
    email: String,
    role: String,
}

async fn create(
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

    let role = parse_role(&body.role, "teams.invitations.create.invalid_role")?;
    teams::validate_managed_member_role(&role).map_err(|error| AppError::Validation {
        op: "teams.invitations.create.owner_role",
        message: error.to_string(),
    })?;

    let db = crate::infra::http::database(&state, "teams.invitations.create.no_database")?;
    let cache = crate::infra::http::cache(&state, "teams.invitations.create.no_cache")?;
    let policy = settings::get_setting(db, "signup.policy")
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.invitations.create.read_policy",
            source,
        })?
        .and_then(|setting| setting.value.as_str().map(str::to_owned));
    let policy = SignupPolicy::parse(policy.as_deref()).map_err(|error| AppError::Internal {
        op: "teams.invitations.create.invalid_policy",
        message: error.to_string(),
    })?;

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
            signup_policy: policy,
        },
    )
    .await
    .map_err(map_invitation_error)?;
    let invitation_role = role_value(&invitation.role);
    let mail_config = state.config.read().unwrap().mail.clone();
    platform_mail::send_invitation_best_effort(
        db,
        mail_config,
        &invitation.email,
        &team,
        invitation_role,
        &token,
    )
    .await;

    Ok(ok_response(CreateResponse {
        invitation: CreateInvitationResponse {
            id: invitation.id,
            team_id: invitation.team_id,
            email: invitation.email.clone(),
            role: invitation_role,
            status: "pending",
            expires_at: invitation.expires_at,
            token,
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
struct CreateInvitationResponse {
    id: uuid::Uuid,
    team_id: uuid::Uuid,
    email: String,
    role: &'static str,
    status: &'static str,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    expires_at: time::OffsetDateTime,
    token: String,
}

#[derive(serde::Serialize)]
struct CreateResponse {
    invitation: CreateInvitationResponse,
}
