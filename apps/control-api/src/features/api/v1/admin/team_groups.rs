use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder,
};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::http::timestamps::ts;
use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        quotas, teams,
    },
    infra::{
        database::entity::{AuditEventResult, team, team_group},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

fn group_view(group: &team_group::Model) -> serde_json::Value {
    let review_policy = group.review_policy.as_ref();
    json!({
        "id": group.id,
        "code": group.code,
        "name": group.name,
        "description": group.description,
        "quota_plan_id": group.quota_plan_id,
        "review_policy": {
            "production": review_policy.and_then(|policy| policy.get("production")),
            "preview": review_policy.and_then(|policy| policy.get("preview")),
        },
        "is_default": group.is_default,
        "created_at": ts(group.created_at),
    })
}

async fn find_group(
    db: &sea_orm::DatabaseConnection,
    group_id: Uuid,
    op: &'static str,
) -> Result<team_group::Model, AppError> {
    team_group::Entity::find()
        .filter(team_group::Column::Id.eq(group_id))
        .filter(team_group::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "team group not found".to_owned(),
        })
}

/// Rejects a quota plan id that does not exist or is disabled, so the error
/// is a 400 instead of a foreign-key 500 (or a silently ignored plan).
async fn validate_plan_reference(
    db: &sea_orm::DatabaseConnection,
    plan_id: Uuid,
    op: &'static str,
) -> Result<(), AppError> {
    let plan = quotas::get_plan(db, plan_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    match plan {
        Some(plan) if plan.enabled => Ok(()),
        Some(_) => Err(AppError::Validation {
            op,
            message: "quota plan is disabled".to_owned(),
        }),
        None => Err(AppError::Validation {
            op,
            message: "quota plan not found".to_owned(),
        }),
    }
}

async fn audit_group_mutation(
    db: &sea_orm::DatabaseConnection,
    actor: Uuid,
    action: &str,
    group_id: Uuid,
    metadata: serde_json::Value,
) {
    let _ = audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(actor),
            actor_node_id: None,
            team_id: None,
            action: action.to_owned(),
            target_type: "team_group".to_owned(),
            target_id: Some(group_id),
            result: AuditEventResult::Success,
            reason: None,
            metadata,
        },
    )
    .await;
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

    let group_ids = groups.iter().map(|group| group.id).collect::<Vec<_>>();
    let mut counts = std::collections::HashMap::new();
    for group_id in &group_ids {
        let count = team::Entity::find()
            .filter(team::Column::GroupId.eq(*group_id))
            .filter(team::Column::DeletedAt.is_null())
            .count(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
        counts.insert(*group_id, count);
    }

    Ok(ok_response(json!({
        "groups": groups
            .iter()
            .map(|group| {
                let mut view = group_view(group);
                view["team_count"] = json!(counts.get(&group.id).copied().unwrap_or(0));
                view
            })
            .collect::<Vec<_>>(),
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
    #[serde(default)]
    pub review_policy: Option<ReviewPolicyOverrideRequest>,
}

#[derive(Deserialize)]
pub struct ReviewPolicyOverrideRequest {
    #[serde(default)]
    pub production: Option<String>,
    #[serde(default)]
    pub preview: Option<String>,
}

fn review_policy_value(
    policy: ReviewPolicyOverrideRequest,
    op: &'static str,
) -> Result<Option<serde_json::Value>, AppError> {
    fn validate_mode(value: Option<String>, op: &'static str) -> Result<Option<String>, AppError> {
        match value.as_deref() {
            None => Ok(None),
            Some("auto" | "manual") => Ok(value),
            Some(_) => Err(AppError::Validation {
                op,
                message: "review policy must be auto, manual, or inherit".to_owned(),
            }),
        }
    }

    let production = validate_mode(policy.production, op)?;
    let preview = validate_mode(policy.preview, op)?;
    if production.is_none() && preview.is_none() {
        return Ok(None);
    }
    let mut value = serde_json::Map::new();
    if let Some(production) = production {
        value.insert("production".to_owned(), json!(production));
    }
    if let Some(preview) = preview {
        value.insert("preview".to_owned(), json!(preview));
    }
    Ok(Some(serde_json::Value::Object(value)))
}

/// POST /api/v1/admin/team-groups
pub async fn create(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
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
    if let Some(plan_id) = body.quota_plan_id {
        validate_plan_reference(db, plan_id, OP).await?;
    }
    let review_policy = body
        .review_policy
        .map(|policy| review_policy_value(policy, OP))
        .transpose()?
        .flatten();

    let now = OffsetDateTime::now_utc();
    let group = team_group::ActiveModel {
        id: Set(Uuid::now_v7()),
        code: Set(code),
        name: Set(body.name.trim().to_owned()),
        description: Set(body.description),
        quota_plan_id: Set(body.quota_plan_id),
        review_policy: Set(review_policy),
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

    audit_group_mutation(
        db,
        data.user_id,
        "team_group.created",
        group.id,
        json!({ "code": group.code, "review_policy": group.review_policy }),
    )
    .await;

    Ok(ok_response(json!({ "group": group_view(&group) })))
}

#[derive(Deserialize)]
pub struct UpdateTeamGroupRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Explicit `null` detaches the quota plan.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub quota_plan_id: Option<Option<Uuid>>,
    #[serde(default)]
    pub review_policy: Option<ReviewPolicyOverrideRequest>,
    #[serde(default)]
    pub is_default: Option<bool>,
}

fn deserialize_double_option<'de, D>(deserializer: D) -> Result<Option<Option<Uuid>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    Option::<Uuid>::deserialize(deserializer).map(Some)
}

/// PATCH /api/v1/admin/team-groups/{group_id}
pub async fn update(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(group_id): Path<Uuid>,
    Json(body): Json<UpdateTeamGroupRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.team_groups.update";
    let db = super::database(&state, OP)?;

    let group = find_group(db, group_id, OP).await?;

    if group.is_default && body.is_default == Some(false) {
        return Err(AppError::Validation {
            op: OP,
            message: "promote another group to default instead of un-defaulting this one"
                .to_owned(),
        });
    }
    if let Some(Some(plan_id)) = body.quota_plan_id {
        validate_plan_reference(db, plan_id, OP).await?;
    }
    let review_policy = body
        .review_policy
        .map(|policy| review_policy_value(policy, OP))
        .transpose()?;

    let promote = body.is_default == Some(true) && !group.is_default;
    let mut active: team_group::ActiveModel = group.into();
    if let Some(name) = body.name.filter(|name| !name.trim().is_empty()) {
        active.name = Set(name.trim().to_owned());
    }
    if let Some(description) = body.description {
        active.description = Set(Some(description));
    }
    if let Some(quota_plan_id) = body.quota_plan_id {
        active.quota_plan_id = Set(quota_plan_id);
    }
    if let Some(review_policy) = review_policy {
        active.review_policy = Set(review_policy);
    }
    let group = active
        .update(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    if promote {
        use sea_orm::sea_query::Expr;
        team_group::Entity::update_many()
            .col_expr(team_group::Column::IsDefault, Expr::value(false))
            .filter(team_group::Column::IsDefault.eq(true))
            .filter(team_group::Column::DeletedAt.is_null())
            .exec(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
        team_group::Entity::update_many()
            .col_expr(team_group::Column::IsDefault, Expr::value(true))
            .filter(team_group::Column::Id.eq(group.id))
            .exec(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
    }

    audit_group_mutation(
        db,
        data.user_id,
        "team_group.updated",
        group.id,
        json!({
            "code": group.code,
            "made_default": promote,
            "review_policy": group.review_policy,
        }),
    )
    .await;

    Ok(ok_response(json!({ "group": group_view(&group) })))
}

/// DELETE /api/v1/admin/team-groups/{group_id}
pub async fn remove(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(group_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.team_groups.remove";
    let db = super::database(&state, OP)?;

    let group = find_group(db, group_id, OP).await?;
    if group.is_default {
        return Err(AppError::Validation {
            op: OP,
            message: "the default team group cannot be deleted".to_owned(),
        });
    }
    let team_count = team::Entity::find()
        .filter(team::Column::GroupId.eq(group.id))
        .filter(team::Column::DeletedAt.is_null())
        .count(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    if team_count > 0 {
        return Err(AppError::Conflict {
            op: OP,
            message: format!(
                "{team_count} team(s) are still assigned to this group; move them first"
            ),
        });
    }

    let code = group.code.clone();
    let mut active: team_group::ActiveModel = group.into();
    active.deleted_at = Set(Some(OffsetDateTime::now_utc()));
    let group = active
        .update(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    audit_group_mutation(
        db,
        data.user_id,
        "team_group.deleted",
        group.id,
        json!({ "code": code }),
    )
    .await;

    Ok(ok_response(json!({ "deleted": true })))
}

#[derive(Deserialize)]
pub struct AssignGroupRequest {
    pub group_id: Uuid,
}

/// POST /api/v1/admin/teams/{team_id}/group
pub async fn assign(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
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
    let group = find_group(db, body.group_id, OP).await?;

    let mut active: team::ActiveModel = target.into();
    active.group_id = Set(Some(group.id));
    let team = active
        .update(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    let _ = audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: Some(team.id),
            action: "team.group_changed".to_owned(),
            target_type: "team".to_owned(),
            target_id: Some(team.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "group_id": group.id, "group_code": group.code }),
        },
    )
    .await;

    Ok(ok_response(json!({
        "team": { "id": team.id, "slug": team.slug, "group_id": team.group_id },
    })))
}
