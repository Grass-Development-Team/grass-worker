use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        quotas,
        teams::{self, TeamListFilter, UpdateTeamParams},
    },
    infra::{
        database::entity::{AuditEventResult, TeamKind, project, team, team_group},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

fn team_view(
    team: &team::Model,
    group: Option<&team_group::Model>,
    member_count: i64,
) -> serde_json::Value {
    json!({
        "id": team.id,
        "slug": team.slug,
        "name": team.name,
        "kind": team.kind.as_str(),
        "group": group.map(|group| json!({
            "id": group.id,
            "code": group.code,
            "name": group.name,
        })),
        "explicit_quota_plan_id": team.explicit_quota_plan_id,
        "member_count": member_count,
        "created_at": team.created_at,
    })
}

async fn load_groups(
    db: &sea_orm::DatabaseConnection,
    ids: Vec<Uuid>,
) -> anyhow::Result<std::collections::HashMap<Uuid, team_group::Model>> {
    if ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    Ok(team_group::Entity::find()
        .filter(team_group::Column::Id.is_in(ids))
        .all(db)
        .await?
        .into_iter()
        .map(|group| (group.id, group))
        .collect())
}

#[derive(Deserialize)]
pub struct ListTeamsQuery {
    pub q: Option<String>,
    pub limit: Option<u64>,
}

/// GET /api/v1/admin/teams
pub async fn list(
    State(state): State<ControlApiState>,
    Query(query): Query<ListTeamsQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.teams.list";
    let db = super::database(&state, OP)?;

    let teams = teams::list_all(
        db,
        TeamListFilter {
            query: query.q,
            limit: query.limit.unwrap_or(100),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    let team_ids = teams.iter().map(|team| team.id).collect::<Vec<_>>();
    let counts = teams::member_counts(db, &team_ids)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let groups = load_groups(db, teams.iter().filter_map(|team| team.group_id).collect())
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(json!({
        "teams": teams
            .iter()
            .map(|team| team_view(
                team,
                team.group_id.and_then(|id| groups.get(&id)),
                counts.get(&team.id).copied().unwrap_or(0),
            ))
            .collect::<Vec<_>>(),
    })))
}

/// GET /api/v1/admin/teams/{team_id}
pub async fn detail(
    State(state): State<ControlApiState>,
    Path(team_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.teams.detail";
    let db = super::database(&state, OP)?;

    let team = teams::get_by_id(db, team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "team not found".to_owned(),
        })?;

    let members = teams::list_members(db, team.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let groups = load_groups(db, team.group_id.into_iter().collect())
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let quota = quotas::resolve_team_quota(db, &team)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let project_count = project::Entity::find()
        .filter(project::Column::TeamId.eq(team.id))
        .filter(project::Column::DeletedAt.is_null())
        .count(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(json!({
        "team": team_view(
            &team,
            team.group_id.and_then(|id| groups.get(&id)),
            members.len() as i64,
        ),
        "members": members
            .iter()
            .map(|(member, user)| json!({
                "user_id": user.id,
                "email": user.email,
                "display_name": user.display_name,
                "role": crate::features::api::v1::teams::role_value(&member.role),
                "joined_at": member.joined_at,
            }))
            .collect::<Vec<_>>(),
        "quota_plan": {
            "id": quota.plan.id,
            "code": quota.plan.code,
            "name": quota.plan.name,
            "source": quota.source.as_str(),
        },
        "project_count": project_count,
    })))
}

#[derive(Deserialize)]
pub struct UpdateTeamRequest {
    pub name: Option<String>,
}

/// PATCH /api/v1/admin/teams/{team_id}
pub async fn update(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(team_id): Path<Uuid>,
    Json(body): Json<UpdateTeamRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.teams.update";
    let db = super::database(&state, OP)?;

    let name = body
        .name
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());
    if name.as_ref().is_some_and(|name| name.chars().count() > 120) {
        return Err(AppError::Validation {
            op: OP,
            message: "team name must not exceed 120 characters".to_owned(),
        });
    }
    let Some(name) = name else {
        return Err(AppError::Validation {
            op: OP,
            message: "nothing to update".to_owned(),
        });
    };

    teams::get_by_id(db, team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "team not found".to_owned(),
        })?;

    let team = teams::update(
        db,
        team_id,
        UpdateTeamParams {
            slug: None,
            name: Some(name),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            team_id: Some(team.id),
            action: "team.updated".to_owned(),
            target_type: "team".to_owned(),
            target_id: Some(team.id),
            result: AuditEventResult::Success,
            reason: Some("updated by platform administrator".to_owned()),
            metadata: json!({ "changed": ["name"] }),
        },
    )
    .await;

    Ok(ok_response(json!({ "team": team_view(&team, None, 0) })))
}

/// DELETE /api/v1/admin/teams/{team_id}
///
/// Soft-deletes a standard team. Personal teams and teams that still own
/// projects are refused.
pub async fn remove(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(team_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.teams.remove";
    let db = super::database(&state, OP)?;

    let team = teams::get_by_id(db, team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "team not found".to_owned(),
        })?;

    if team.kind == TeamKind::Personal {
        return Err(AppError::Validation {
            op: OP,
            message: "personal teams cannot be deleted".to_owned(),
        });
    }
    let project_count = project::Entity::find()
        .filter(project::Column::TeamId.eq(team.id))
        .filter(project::Column::DeletedAt.is_null())
        .count(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    if project_count > 0 {
        return Err(AppError::Conflict {
            op: OP,
            message: format!(
                "team still owns {project_count} project(s); delete or transfer them first"
            ),
        });
    }

    teams::soft_delete(db, team.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            team_id: Some(team.id),
            action: "team.deleted".to_owned(),
            target_type: "team".to_owned(),
            target_id: Some(team.id),
            result: AuditEventResult::Success,
            reason: Some("deleted by platform administrator".to_owned()),
            metadata: json!({ "slug": team.slug }),
        },
    )
    .await;

    Ok(ok_response(json!({ "deleted": true })))
}

#[derive(Deserialize)]
pub struct SetQuotaPlanRequest {
    /// `null` clears the override so the team inherits its group plan.
    pub plan_id: Option<Uuid>,
}

/// POST /api/v1/admin/teams/{team_id}/quota-plan
///
/// Sets or clears the explicit per-team quota plan override, which wins
/// over the team group's plan during resolution.
pub async fn set_quota_plan(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(team_id): Path<Uuid>,
    Json(body): Json<SetQuotaPlanRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.teams.set_quota_plan";
    let db = super::database(&state, OP)?;

    let target = teams::get_by_id(db, team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "team not found".to_owned(),
        })?;

    if let Some(plan_id) = body.plan_id {
        let plan = crate::domain::quotas::get_plan(db, plan_id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        match plan {
            Some(plan) if plan.enabled => {}
            Some(_) => {
                return Err(AppError::Validation {
                    op: OP,
                    message: "quota plan is disabled".to_owned(),
                });
            }
            None => {
                return Err(AppError::Validation {
                    op: OP,
                    message: "quota plan not found".to_owned(),
                });
            }
        }
    }

    use sea_orm::{ActiveModelTrait, ActiveValue::Set};
    let mut active: team::ActiveModel = target.into();
    active.explicit_quota_plan_id = Set(body.plan_id);
    let team = active
        .update(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            team_id: Some(team.id),
            action: "team.quota_plan_overridden".to_owned(),
            target_type: "team".to_owned(),
            target_id: Some(team.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "plan_id": body.plan_id }),
        },
    )
    .await;

    Ok(ok_response(json!({
        "team": {
            "id": team.id,
            "slug": team.slug,
            "explicit_quota_plan_id": team.explicit_quota_plan_id,
        },
    })))
}
