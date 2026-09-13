pub(crate) mod audit_events;
pub(crate) mod avatar;
pub(crate) mod invitation_candidates;
pub(crate) mod invitations;
pub(crate) mod members;
pub(crate) mod quota;
pub(crate) mod source_credentials;
pub(crate) mod ssh_host_keys;

use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::teams::{self, UpdateTeamParams},
    infra::{
        error::{AppError, ok_response},
        http::extractors::TeamRole,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/teams/{team_id}", axum::routing::get(detail).patch(update))
        .merge(audit_events::router())
        .merge(avatar::router())
        .merge(invitation_candidates::router())
        .merge(invitations::router())
        .merge(members::router())
        .merge(quota::router())
        .merge(source_credentials::router())
        .merge(ssh_host_keys::router())
}

pub(crate) fn team_avatar_url(team_id: Uuid, version: Option<Uuid>) -> Option<String> {
    version.map(|version| format!("/api/v1/avatars/teams/{team_id}/{version}/avatar.webp"))
}

pub(crate) fn validate_required(value: &str, op: &'static str, name: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::Validation {
            op,
            message: format!("{name} is required"),
        });
    }
    Ok(())
}

pub(crate) fn normalize_slug(value: &str, op: &'static str) -> Result<String, AppError> {
    grass_validator::normalize_slug(value).map_err(|error| AppError::Validation {
        op,
        message: error.to_string(),
    })
}

pub(crate) fn map_team_write_error(source: anyhow::Error, op: &'static str) -> AppError {
    if crate::infra::database::is_unique_violation(&source) {
        AppError::Conflict {
            op,
            message: "team slug is already in use".to_owned(),
        }
    } else {
        AppError::Infrastructure { op, source }
    }
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

pub(crate) fn kind_value(kind: &crate::infra::database::entity::TeamKind) -> &'static str {
    use crate::infra::database::entity::TeamKind;

    match kind {
        TeamKind::Personal => "personal",
        TeamKind::Team => "team",
    }
}

pub async fn detail(
    State(state): State<ControlApiState>,
    team_role: TeamRole,
) -> Result<impl IntoResponse, AppError> {
    let db = crate::infra::http::database(&state, "teams.detail.no_database")?;
    let team = teams::get_by_id(db, team_role.team_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.detail",
            source,
        })?
        .ok_or_else(|| AppError::NotFound {
            op: "teams.detail.not_found",
            message: "team not found".to_owned(),
        })?;

    Ok(ok_response(DetailResponse {
        team: DetailTeamResponse {
            id: team.id,
            slug: team.slug.clone(),
            name: team.name.clone(),
            avatar_url: team_avatar_url(team.id, team.avatar_version),
            kind: kind_value(&team.kind),
            owner_user_id: team.owner_user_id,
            group_id: team.group_id,
            role: role_value(&team_role.role),
        },
    }))
}

#[derive(Deserialize)]
pub struct UpdateTeamRequest {
    pub name: Option<String>,
    pub slug: Option<String>,
}

pub async fn update(
    State(state): State<ControlApiState>,
    team_role: TeamRole,
    Json(body): Json<UpdateTeamRequest>,
) -> Result<impl IntoResponse, AppError> {
    team_role.require_owner("teams.update.owner_required")?;

    if let Some(name) = &body.name {
        validate_required(name, "teams.update.invalid_name", "name")?;
        if name.trim().chars().count() > 160 {
            return Err(AppError::Validation {
                op: "teams.update.invalid_name",
                message: "team name must not exceed 160 characters".to_owned(),
            });
        }
    }

    let slug = body
        .slug
        .as_deref()
        .map(|slug| normalize_slug(slug, "teams.update.invalid_slug"))
        .transpose()?;

    let db = crate::infra::http::database(&state, "teams.update.no_database")?;
    let team = teams::update(
        db,
        team_role.team_id,
        UpdateTeamParams {
            slug,
            name: body.name.map(|name| name.trim().to_owned()),
        },
    )
    .await
    .map_err(|source| map_team_write_error(source, "teams.update"))?;

    Ok(ok_response(UpdateResponse {
        team: UpdateTeamResponse {
            id: team.id,
            slug: team.slug.clone(),
            name: team.name.clone(),
            avatar_url: team_avatar_url(team.id, team.avatar_version),
            kind: kind_value(&team.kind),
            owner_user_id: team.owner_user_id,
            group_id: team.group_id,
        },
    }))
}

#[derive(serde::Serialize)]
struct DetailTeamResponse {
    id: uuid::Uuid,
    slug: String,
    name: String,
    avatar_url: Option<String>,
    kind: &'static str,
    owner_user_id: Option<uuid::Uuid>,
    group_id: Option<uuid::Uuid>,
    role: &'static str,
}

#[derive(serde::Serialize)]
struct DetailResponse {
    team: DetailTeamResponse,
}

#[derive(serde::Serialize)]
struct UpdateTeamResponse {
    id: uuid::Uuid,
    slug: String,
    name: String,
    avatar_url: Option<String>,
    kind: &'static str,
    owner_user_id: Option<uuid::Uuid>,
    group_id: Option<uuid::Uuid>,
}

#[derive(serde::Serialize)]
struct UpdateResponse {
    team: UpdateTeamResponse,
}
