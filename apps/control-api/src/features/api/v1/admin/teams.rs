pub(crate) mod batch;
pub(crate) mod by_team_id;

use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::teams::{self, TeamListFilter},
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, TeamKind, team, team_group},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/teams", axum::routing::get(list).post(create))
        .merge(batch::router())
        .merge(by_team_id::router())
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

#[derive(Deserialize)]
pub struct ListTeamsQuery {
    pub q: Option<String>,
    pub limit: Option<u64>,
    pub kind: Option<String>,
    pub group_id: Option<Uuid>,
    pub quota_plan_id: Option<Uuid>,
}

fn parse_team_kind_filter(
    value: Option<&str>,
    op: &'static str,
) -> Result<Option<TeamKind>, AppError> {
    value
        .map(|value| match value {
            "personal" => Ok(TeamKind::Personal),
            "team" => Ok(TeamKind::Team),
            _ => Err(AppError::Validation {
                op,
                message: "kind must be personal or team".to_owned(),
            }),
        })
        .transpose()
}

/// GET /api/v1/admin/teams
pub async fn list(
    State(state): State<ControlApiState>,
    Query(query): Query<ListTeamsQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.teams.list";
    let db = crate::infra::http::database(&state, OP)?;
    let kind = parse_team_kind_filter(query.kind.as_deref(), OP)?;

    let teams = teams::list_all(
        db,
        TeamListFilter {
            query: query.q,
            kind,
            group_id: query.group_id,
            quota_plan_id: query.quota_plan_id,
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

    Ok(ok_response(ListResponse {
        teams: teams
            .iter()
            .map(|team| {
                team_view(
                    team,
                    team.group_id.and_then(|id| groups.get(&id)),
                    counts.get(&team.id).copied().unwrap_or(0),
                )
            })
            .collect::<Vec<_>>(),
    }))
}

#[derive(Deserialize)]
pub struct CreateTeamRequest {
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
    pub owner_user_id: Uuid,
}

/// POST /api/v1/admin/teams — creates a standard team owned by an existing
/// active user.
pub async fn create(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Json(body): Json<CreateTeamRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.teams.create";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;

    let name = body.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(AppError::Validation {
            op: OP,
            message: "team name must contain between 1 and 120 characters".to_owned(),
        });
    }
    let slug_source = body.slug.filter(|slug| !slug.trim().is_empty());
    let slug = grass_validator::normalize_slug(slug_source.as_deref().unwrap_or(&name)).map_err(
        |error| AppError::Validation {
            op: OP,
            message: error.to_string(),
        },
    )?;

    let owner = crate::domain::users::get_user_by_id(db, body.owner_user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::Validation {
            op: OP,
            message: "owner user not found".to_owned(),
        })?;
    if owner.status != crate::infra::database::entity::UserStatus::Active {
        return Err(AppError::Validation {
            op: OP,
            message: "owner account is disabled".to_owned(),
        });
    }

    let team = teams::create_team_with_connection(
        db,
        crate::domain::teams::CreateTeamParams {
            slug,
            name,
            kind: TeamKind::Team,
            owner_user_id: owner.id,
            group_id: None,
        },
    )
    .await
    .map_err(|source| {
        if crate::infra::database::is_unique_violation(&source) {
            AppError::Conflict {
                op: OP,
                message: "team slug is already in use".to_owned(),
            }
        } else {
            AppError::Infrastructure { op: OP, source }
        }
    })?;

    audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: Some(team.id),
            action: "team.created".to_owned(),
            target_type: "team".to_owned(),
            target_id: Some(team.id),
            result: AuditEventResult::Success,
            reason: Some("created by platform administrator".to_owned()),
            metadata: json!({ "slug": team.slug, "owner": owner.email }),
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

    Ok(ok_response(CreateResponse {
        team: team_view(&team, None, 1),
    }))
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
struct ListResponse {
    teams: Vec<TeamResponse>,
}

#[derive(serde::Serialize)]
struct CreateResponse {
    team: TeamResponse,
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::infra::database::entity::TeamKind;
    #[test]
    fn team_kind_filter_is_typed() {
        assert_eq!(
            parse_team_kind_filter(Some("team"), "test.teams").unwrap(),
            Some(TeamKind::Team)
        );
        assert!(parse_team_kind_filter(Some("enterprise"), "test.teams").is_err());
    }
}
