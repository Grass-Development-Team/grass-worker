use axum::{Json, extract::Query, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;

use crate::infra::http::timestamps::ts;
use crate::{
    domain::{
        platform_mail,
        quotas::QuotaDimension,
        teams::{self, AcceptInvitationParams, CreateInvitationParams, InvitationError},
        users,
    },
    infra::{
        error::{AppError, ok_response},
        http::extractors::{Session, TeamRole, session::OptionalSession},
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

    let db = super::database(&state, "teams.invitations.preflight.no_database")?;
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

    Ok(ok_response(json!({
        "team": { "id": team.id, "name": team.name },
        "role": super::role_value(&invitation.role),
        "expires_at": ts(invitation.expires_at),
        "status": status,
        "email_matches_current_user": email_matches_current_user,
        "can_accept": can_accept,
    })))
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
    let invitation_role = super::role_value(&invitation.role);
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

    Ok(ok_response(json!({
        "invitation": {
            "id": invitation.id,
            "team_id": invitation.team_id,
            "email": invitation.email,
            "role": invitation_role,
            "status": "pending",
            "expires_at": ts(invitation.expires_at),
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
            "joined_at": ts(member.joined_at),
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

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use sea_orm::MockDatabase;
    use time::{Duration, OffsetDateTime};
    use tower::ServiceExt;
    use uuid::Uuid;

    use crate::{
        infra::{
            config::ControlApiConfig,
            database::entity::{
                TeamInvitationStatus, TeamKind, TeamMemberRole, team, team_invitation,
            },
        },
        state::ControlApiState,
    };

    #[tokio::test]
    async fn preflight_is_public_and_describes_the_invitation() {
        let now = OffsetDateTime::now_utc();
        let team_id = Uuid::now_v7();
        let token = "invitation-secret";
        let invitation = team_invitation::Model {
            id: Uuid::now_v7(),
            team_id,
            email: "invitee@example.com".to_owned(),
            role: TeamMemberRole::Member,
            status: TeamInvitationStatus::Pending,
            invited_by_user_id: None,
            token_hash: Some(crate::domain::teams::invitation_token_hash(token)),
            expires_at: now + Duration::days(7),
            accepted_at: None,
            created_at: now,
            updated_at: now,
        };
        let team = team::Model {
            id: team_id,
            slug: "acme".to_owned(),
            name: "Acme Team".to_owned(),
            kind: TeamKind::Team,
            group_id: None,
            explicit_quota_plan_id: None,
            owner_user_id: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };
        let database = MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[invitation]])
            .append_query_results([[team]])
            .into_connection();
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        assert!(state.database.set(database).is_ok());

        let response = super::super::router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/team-invitations/preflight?token={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["data"]["team"]["name"], "Acme Team");
        assert_eq!(body["data"]["role"], "member");
        assert_eq!(body["data"]["status"], "pending");
        assert_eq!(
            body["data"]["email_matches_current_user"],
            serde_json::Value::Null
        );
        assert_eq!(body["data"]["can_accept"], false);
    }
}
