pub(crate) mod group;
pub(crate) mod quota_plan;

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{
        quotas,
        teams::{self, UpdateTeamParams},
    },
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, project, team, team_group},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route(
            "/teams/{team_id}",
            axum::routing::delete(remove).get(detail).patch(update),
        )
        .merge(group::router())
        .merge(quota_plan::router())
}

fn team_view(
    team: &team::Model,
    group: Option<&team_group::Model>,
    member_count: i64,
) -> TeamResponse {
    TeamResponse {
        id: team.id,
        slug: team.slug.clone(),
        name: team.name.clone(),
        kind: team.kind.as_str(),
        group: group.map(|group| TeamGroupResponse {
            id: group.id,
            code: group.code.clone(),
            name: group.name.clone(),
        }),
        explicit_quota_plan_id: team.explicit_quota_plan_id,
        member_count,
        created_at: team.created_at,
    }
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

/// GET /api/v1/admin/teams/{team_id}
async fn detail(
    State(state): State<ControlApiState>,
    Path(team_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.teams.detail";
    let db = crate::infra::http::database(&state, OP)?;

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

    Ok(ok_response(DetailResponse {
        team: team_view(
            &team,
            team.group_id.and_then(|id| groups.get(&id)),
            members.len() as i64,
        ),
        members: members
            .iter()
            .map(|(member, user)| DetailMembersResponse {
                user_id: user.id,
                email: user.email.clone(),
                display_name: user.display_name.clone(),
                role: role_value(&member.role),
                joined_at: member.joined_at,
            })
            .collect::<Vec<_>>(),
        quota_plan: DetailQuotaPlanResponse {
            id: quota.plan.id,
            code: quota.plan.code.clone(),
            name: quota.plan.name.clone(),
            source: (quota.source.as_str()).to_owned(),
        },
        project_count,
    }))
}

#[derive(Deserialize)]
struct UpdateTeamRequest {
    name: Option<String>,
}

/// PATCH /api/v1/admin/teams/{team_id}
async fn update(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(team_id): Path<Uuid>,
    Json(body): Json<UpdateTeamRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.teams.update";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;

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

    audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: Some(team.id),
            action: "team.updated".to_owned(),
            target_type: "team".to_owned(),
            target_id: Some(team.id),
            result: AuditEventResult::Success,
            reason: Some("updated by platform administrator".to_owned()),
            metadata: json!({ "changed": ["name"] }),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(UpdateResponse {
        team: team_view(&team, None, 0),
    }))
}

/// DELETE /api/v1/admin/teams/{team_id}
///
/// Soft-deletes a standard team. Personal teams and teams that still own
/// projects are refused.
async fn remove(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(team_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    crate::domain::admin_teams::remove(&state, data.user_id, team_id).await?;
    Ok(ok_response(RemoveResponse { deleted: true }))
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

#[derive(serde::Serialize)]
struct TeamGroupResponse {
    id: uuid::Uuid,
    code: String,
    name: String,
}

#[derive(serde::Serialize)]
struct TeamResponse {
    id: uuid::Uuid,
    slug: String,
    name: String,
    kind: &'static str,
    group: Option<TeamGroupResponse>,
    explicit_quota_plan_id: Option<uuid::Uuid>,
    member_count: i64,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct DetailMembersResponse {
    user_id: uuid::Uuid,
    email: String,
    display_name: Option<String>,
    role: &'static str,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    joined_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct DetailQuotaPlanResponse {
    id: uuid::Uuid,
    code: String,
    name: String,
    source: String,
}

#[derive(serde::Serialize)]
struct DetailResponse {
    team: TeamResponse,
    members: Vec<DetailMembersResponse>,
    quota_plan: DetailQuotaPlanResponse,
    project_count: u64,
}

#[derive(serde::Serialize)]
struct UpdateResponse {
    team: TeamResponse,
}

#[derive(serde::Serialize)]
struct RemoveResponse {
    deleted: bool,
}
