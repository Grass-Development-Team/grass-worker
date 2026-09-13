use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::quotas,
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, team, team_group},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/team-groups/{group_id}",
        axum::routing::delete(remove).patch(update),
    )
}

fn group_view(group: &team_group::Model) -> TeamGroupResponse {
    let review_policy = group.review_policy.as_ref();
    TeamGroupResponse {
        id: group.id,
        code: group.code.clone(),
        name: group.name.clone(),
        description: group.description.clone(),
        quota_plan_id: group.quota_plan_id,
        review_policy: TeamGroupReviewPolicyResponse {
            production: review_policy
                .and_then(|policy| policy.get("production"))
                .cloned(),
            preview: review_policy
                .and_then(|policy| policy.get("preview"))
                .cloned(),
            domain: review_policy
                .and_then(|policy| policy.get("domain"))
                .cloned(),
        },
        is_default: group.is_default,
        created_at: group.created_at,
    }
}

async fn find_group(
    db: &impl sea_orm::ConnectionTrait,
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
    db: &impl sea_orm::ConnectionTrait,
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
    db: &impl audits::AuditConnection,
    actor: Uuid,
    action: &str,
    group_id: Uuid,
    metadata: serde_json::Value,
) -> anyhow::Result<()> {
    audits::create_platform_audit_event(
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
    .await
}

#[derive(Deserialize)]
pub struct ReviewPolicyOverrideRequest {
    #[serde(default)]
    pub production: Option<String>,
    #[serde(default)]
    pub preview: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
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
    let domain = validate_mode(policy.domain, op)?;
    if production.is_none() && preview.is_none() && domain.is_none() {
        return Ok(None);
    }
    let mut value = serde_json::Map::new();
    if let Some(production) = production {
        value.insert("production".to_owned(), json!(production));
    }
    if let Some(preview) = preview {
        value.insert("preview".to_owned(), json!(preview));
    }
    if let Some(domain) = domain {
        value.insert("domain".to_owned(), json!(domain));
    }
    Ok(Some(serde_json::Value::Object(value)))
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
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;

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
        group: group_view(&group),
    }))
}

/// DELETE /api/v1/admin/team-groups/{group_id}
pub async fn remove(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(group_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.team_groups.remove";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;

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
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(RemoveResponse { deleted: true }))
}

#[derive(serde::Serialize)]
struct TeamGroupReviewPolicyResponse {
    production: Option<serde_json::Value>,
    preview: Option<serde_json::Value>,
    domain: Option<serde_json::Value>,
}

#[derive(serde::Serialize)]
struct TeamGroupResponse {
    id: uuid::Uuid,
    code: String,
    name: String,
    description: Option<String>,
    quota_plan_id: Option<uuid::Uuid>,
    review_policy: TeamGroupReviewPolicyResponse,
    is_default: bool,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct UpdateResponse {
    group: TeamGroupResponse,
}

#[derive(serde::Serialize)]
struct RemoveResponse {
    deleted: bool,
}
