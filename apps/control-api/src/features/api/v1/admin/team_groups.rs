use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::teams,
    infra::{
        database::entity::{team, team_group},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

fn group_view(group: &team_group::Model) -> serde_json::Value {
    json!({
        "id": group.id,
        "code": group.code,
        "name": group.name,
        "description": group.description,
        "quota_plan_id": group.quota_plan_id,
        "is_default": group.is_default,
        "created_at": group.created_at,
    })
}

/// GET /api/v1/admin/team-groups
pub async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.team_groups.list";
    let db = super::database(&state, OP)?;

    let groups = team_group::Entity::find()
        .filter(team_group::Column::DeletedAt.is_null())
        .order_by_asc(team_group::Column::Code)
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(json!({
        "groups": groups.iter().map(group_view).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct CreateTeamGroupRequest {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub quota_plan_id: Option<Uuid>,
}

/// POST /api/v1/admin/team-groups
pub async fn create(
    State(state): State<ControlApiState>,
    Json(body): Json<CreateTeamGroupRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.team_groups.create";
    let db = super::database(&state, OP)?;

    let code =
        grass_validator::normalize_slug(&body.code).map_err(|error| AppError::Validation {
            op: OP,
            message: error.to_string(),
        })?;
    if body.name.trim().is_empty() {
        return Err(AppError::Validation {
            op: OP,
            message: "name is required".to_owned(),
        });
    }

    let now = OffsetDateTime::now_utc();
    let group = team_group::ActiveModel {
        id: Set(Uuid::now_v7()),
        code: Set(code),
        name: Set(body.name.trim().to_owned()),
        description: Set(body.description),
        quota_plan_id: Set(body.quota_plan_id),
        is_default: Set(false),
        deleted_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .map_err(|source| {
        let source = anyhow::Error::from(source);
        if crate::infra::database::is_unique_violation(&source) {
            AppError::Conflict {
                op: OP,
                message: "team group code is already in use".to_owned(),
            }
        } else {
            AppError::Infrastructure { op: OP, source }
        }
    })?;

    Ok(ok_response(json!({ "group": group_view(&group) })))
}

#[derive(Deserialize)]
pub struct UpdateTeamGroupRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub quota_plan_id: Option<Uuid>,
}

/// PATCH /api/v1/admin/team-groups/{group_id}
pub async fn update(
    State(state): State<ControlApiState>,
    Path(group_id): Path<Uuid>,
    Json(body): Json<UpdateTeamGroupRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.team_groups.update";
    let db = super::database(&state, OP)?;

    let group = team_group::Entity::find()
        .filter(team_group::Column::Id.eq(group_id))
        .filter(team_group::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "team group not found".to_owned(),
        })?;

    let mut active: team_group::ActiveModel = group.into();
    if let Some(name) = body.name.filter(|name| !name.trim().is_empty()) {
        active.name = Set(name.trim().to_owned());
    }
    if let Some(description) = body.description {
        active.description = Set(Some(description));
    }
    if let Some(quota_plan_id) = body.quota_plan_id {
        active.quota_plan_id = Set(Some(quota_plan_id));
    }
    let group = active
        .update(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(json!({ "group": group_view(&group) })))
}

#[derive(Deserialize)]
pub struct AssignGroupRequest {
    pub group_id: Uuid,
}

/// POST /api/v1/admin/teams/{team_id}/group
pub async fn assign(
    State(state): State<ControlApiState>,
    Path(team_id): Path<Uuid>,
    Json(body): Json<AssignGroupRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.teams.assign_group";
    let db = super::database(&state, OP)?;

    let target = teams::get_by_id(db, team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "team not found".to_owned(),
        })?;
    let group = team_group::Entity::find()
        .filter(team_group::Column::Id.eq(body.group_id))
        .filter(team_group::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "team group not found".to_owned(),
        })?;

    let mut active: team::ActiveModel = target.into();
    active.group_id = Set(Some(group.id));
    let team = active
        .update(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(json!({
        "team": { "id": team.id, "slug": team.slug, "group_id": team.group_id },
    })))
}
