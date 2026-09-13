pub(crate) mod by_team_id;

use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::teams::{self, CreateTeamParams},
    infra::{
        database::entity::TeamKind,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/teams", axum::routing::get(list).post(create))
        .merge(by_team_id::router())
}

fn team_avatar_url(team_id: Uuid, version: Option<Uuid>) -> Option<String> {
    version.map(|version| format!("/api/v1/avatars/teams/{team_id}/{version}/avatar.webp"))
}

fn validate_required(value: &str, op: &'static str, name: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::Validation {
            op,
            message: format!("{name} is required"),
        });
    }
    Ok(())
}

fn normalize_slug(value: &str, op: &'static str) -> Result<String, AppError> {
    grass_validator::normalize_slug(value).map_err(|error| AppError::Validation {
        op,
        message: error.to_string(),
    })
}

fn map_team_write_error(source: anyhow::Error, op: &'static str) -> AppError {
    if crate::infra::database::is_unique_violation(&source) {
        AppError::Conflict {
            op,
            message: "team slug is already in use".to_owned(),
        }
    } else {
        AppError::Infrastructure { op, source }
    }
}

fn kind_value(kind: &crate::infra::database::entity::TeamKind) -> &'static str {
    use crate::infra::database::entity::TeamKind;

    match kind {
        TeamKind::Personal => "personal",
        TeamKind::Team => "team",
    }
}

#[derive(Deserialize)]
struct CreateTeamRequest {
    name: String,
    slug: String,
}

async fn create(
    State(state): State<ControlApiState>,
    session: Session,
    Json(body): Json<CreateTeamRequest>,
) -> Result<impl IntoResponse, AppError> {
    validate_required(&body.name, "teams.create.invalid_name", "name")?;
    if body.name.trim().chars().count() > 160 {
        return Err(AppError::Validation {
            op: "teams.create.invalid_name",
            message: "team name must not exceed 160 characters".to_owned(),
        });
    }
    let slug = normalize_slug(&body.slug, "teams.create.invalid_slug")?;

    let db = crate::infra::http::database(&state, "teams.create.no_database")?;
    let team = teams::create_team(
        db,
        CreateTeamParams {
            slug,
            name: body.name.trim().to_owned(),
            kind: TeamKind::Team,
            owner_user_id: session.data.user_id,
            group_id: None,
        },
    )
    .await
    .map_err(|source| map_team_write_error(source, "teams.create"))?;

    Ok(ok_response(CreateResponse {
        team: CreateTeamResponse {
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

async fn list(
    State(state): State<ControlApiState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    let db = crate::infra::http::database(&state, "teams.list.no_database")?;
    let teams = teams::list_for_user(db, session.data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.list",
            source,
        })?;

    Ok(ok_response(ListResponse {
        teams: teams
            .into_iter()
            .map(|team| ListTeamsResponse {
                id: team.id,
                slug: team.slug.clone(),
                name: team.name.clone(),
                avatar_url: team_avatar_url(team.id, team.avatar_version),
                kind: kind_value(&team.kind),
                owner_user_id: team.owner_user_id,
                group_id: team.group_id,
            })
            .collect::<Vec<_>>(),
    }))
}

#[derive(serde::Serialize)]
struct CreateTeamResponse {
    id: uuid::Uuid,
    slug: String,
    name: String,
    avatar_url: Option<String>,
    kind: &'static str,
    owner_user_id: Option<uuid::Uuid>,
    group_id: Option<uuid::Uuid>,
}

#[derive(serde::Serialize)]
struct CreateResponse {
    team: CreateTeamResponse,
}

#[derive(serde::Serialize)]
struct ListTeamsResponse {
    id: uuid::Uuid,
    slug: String,
    name: String,
    avatar_url: Option<String>,
    kind: &'static str,
    owner_user_id: Option<uuid::Uuid>,
    group_id: Option<uuid::Uuid>,
}

#[derive(serde::Serialize)]
struct ListResponse {
    teams: Vec<ListTeamsResponse>,
}
