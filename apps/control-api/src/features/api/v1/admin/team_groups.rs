pub(crate) mod by_group_id;

use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder,
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
    axum::Router::new()
        .route("/team-groups", axum::routing::get(list).post(create))
        .merge(by_group_id::router())
}

fn group_view(group: &team_group::Model) -> TeamGroupResponse {
    let review_policy = group.review_policy.as_ref();
    TeamGroupResponse {
        team_count: None,
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

/// GET /api/v1/admin/team-groups
pub async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.team_groups.list";
    let db = crate::infra::http::database(&state, OP)?;

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

    Ok(ok_response(ListResponse {
        groups: groups
            .iter()
            .map(|group| {
                let mut view = group_view(group);
                view.team_count = Some(counts.get(&group.id).copied().unwrap_or(0));
                view
            })
            .collect::<Vec<_>>(),
    }))
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

/// POST /api/v1/admin/team-groups
pub async fn create(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Json(body): Json<CreateTeamGroupRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.team_groups.create";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;

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
        group: group_view(&group),
    }))
}

#[derive(serde::Serialize)]
struct TeamGroupReviewPolicyResponse {
    production: Option<serde_json::Value>,
    preview: Option<serde_json::Value>,
    domain: Option<serde_json::Value>,
}

#[derive(serde::Serialize)]
struct TeamGroupResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    team_count: Option<u64>,
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
struct ListResponse {
    groups: Vec<TeamGroupResponse>,
}

#[derive(serde::Serialize)]
struct CreateResponse {
    group: TeamGroupResponse,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_policy_accepts_a_domain_override() {
        let request: ReviewPolicyOverrideRequest = serde_json::from_value(serde_json::json!({
            "domain": "manual",
        }))
        .unwrap();

        let value = review_policy_value(request, "test.team_groups.review_policy").unwrap();

        assert_eq!(value, Some(serde_json::json!({ "domain": "manual" })));
    }
}
