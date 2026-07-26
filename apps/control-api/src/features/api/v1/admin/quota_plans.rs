use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::TransactionTrait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        quotas::{self, CreatePlanParams, QuotaDimension, UpdatePlanParams},
    },
    infra::{
        database::entity::{AuditEventResult, QuotaPeriod, quota_limit, quota_plan},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct QuotaLimitInput {
    pub dimension: String,
    /// `null` removes the limit row (unlimited).
    pub limit_value: Option<i64>,
}

#[derive(Deserialize)]
pub struct CreateQuotaPlanRequest {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub limits: Vec<QuotaLimitInput>,
}

#[derive(Deserialize)]
pub struct UpdateQuotaPlanRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub is_default: Option<bool>,
    #[serde(default)]
    pub limits: Vec<QuotaLimitInput>,
}

#[derive(Serialize)]
struct QuotaPlanView {
    id: Uuid,
    code: String,
    name: String,
    description: Option<String>,
    is_default: bool,
    enabled: bool,
    limits: Vec<QuotaLimitView>,
}

#[derive(Serialize)]
struct QuotaLimitView {
    dimension: String,
    limit_value: i64,
    period: &'static str,
}

fn plan_view(plan: quota_plan::Model, limits: Vec<quota_limit::Model>) -> QuotaPlanView {
    QuotaPlanView {
        id: plan.id,
        code: plan.code,
        name: plan.name,
        description: plan.description,
        is_default: plan.is_default,
        enabled: plan.enabled,
        limits: limits
            .into_iter()
            .map(|limit| QuotaLimitView {
                dimension: limit.dimension,
                limit_value: limit.limit_value,
                period: match limit.period {
                    QuotaPeriod::None => "none",
                    QuotaPeriod::Monthly => "monthly",
                },
            })
            .collect(),
    }
}

/// Bounded limits to upsert plus dimensions whose rows should be removed.
type ParsedLimits = (Vec<(QuotaDimension, i64)>, Vec<QuotaDimension>);

/// Splits the request limits into "set to value" and "remove row".
fn parse_limits(limits: Vec<QuotaLimitInput>, op: &'static str) -> Result<ParsedLimits, AppError> {
    let mut set = Vec::new();
    let mut remove = Vec::new();
    for limit in limits {
        let dimension =
            QuotaDimension::parse(&limit.dimension).ok_or_else(|| AppError::Validation {
                op,
                message: format!("unknown quota dimension: {}", limit.dimension),
            })?;
        match limit.limit_value {
            Some(value) => set.push((dimension, value)),
            None => remove.push(dimension),
        }
    }
    Ok((set, remove))
}

async fn audit_plan_mutation(
    db: &sea_orm::DatabaseConnection,
    actor: Uuid,
    action: &str,
    plan_id: Uuid,
    metadata: serde_json::Value,
) {
    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(actor),
            team_id: None,
            action: action.to_owned(),
            target_type: "quota_plan".to_owned(),
            target_id: Some(plan_id),
            result: AuditEventResult::Success,
            reason: None,
            metadata,
        },
    )
    .await;
}

/// GET /api/v1/admin/quota-plans
pub async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.quota_plans.list";
    let db = super::database(&state, OP)?;
    let plans = quotas::list_plans(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(json!({
        "plans": plans
            .into_iter()
            .map(|(plan, limits)| plan_view(plan, limits))
            .collect::<Vec<_>>(),
    })))
}

/// POST /api/v1/admin/quota-plans
pub async fn create(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Json(body): Json<CreateQuotaPlanRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.quota_plans.create";
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
    let (limits, _removed) = parse_limits(body.limits, OP)?;

    let db = super::database(&state, OP)?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let plan = quotas::create_plan(
        &transaction,
        CreatePlanParams {
            code,
            name: body.name.trim().to_owned(),
            description: body.description,
            limits,
        },
    )
    .await
    .map_err(|source| {
        if crate::infra::database::is_unique_violation(&source) {
            AppError::Conflict {
                op: OP,
                message: "quota plan code is already in use".to_owned(),
            }
        } else {
            AppError::Infrastructure { op: OP, source }
        }
    })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    audit_plan_mutation(
        db,
        data.user_id,
        "quota_plan.created",
        plan.id,
        json!({ "code": plan.code }),
    )
    .await;

    Ok(ok_response(json!({
        "plan": { "id": plan.id, "code": plan.code, "name": plan.name },
    })))
}

/// PATCH /api/v1/admin/quota-plans/{plan_id}
pub async fn update(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(plan_id): Path<Uuid>,
    Json(body): Json<UpdateQuotaPlanRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.quota_plans.update";
    let (limits, removed) = parse_limits(body.limits, OP)?;

    let db = super::database(&state, OP)?;
    let current = quotas::get_plan(db, plan_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "quota plan not found".to_owned(),
        })?;

    // The default plan is the resolution fallback for every team without a
    // group plan; disabling or silently un-defaulting it would 500 all
    // quota-charged requests.
    let stays_default = body.is_default.unwrap_or(current.is_default);
    let stays_enabled = body.enabled.unwrap_or(current.enabled);
    if current.is_default && body.is_default == Some(false) {
        return Err(AppError::Validation {
            op: OP,
            message: "promote another plan to default instead of un-defaulting this one".to_owned(),
        });
    }
    if stays_default && !stays_enabled {
        return Err(AppError::Validation {
            op: OP,
            message: "the default quota plan must stay enabled".to_owned(),
        });
    }
    if body.is_default == Some(true) && !stays_enabled {
        return Err(AppError::Validation {
            op: OP,
            message: "only an enabled plan can become the default".to_owned(),
        });
    }

    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let plan = quotas::update_plan(
        &transaction,
        plan_id,
        UpdatePlanParams {
            name: body.name,
            description: body.description,
            enabled: body.enabled,
            limits,
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?
    .ok_or_else(|| AppError::NotFound {
        op: OP,
        message: "quota plan not found".to_owned(),
    })?;
    for dimension in &removed {
        quotas::delete_limit(&transaction, plan_id, *dimension)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    }
    if body.is_default == Some(true) && !current.is_default {
        quotas::set_default_plan(&transaction, plan_id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    }
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    audit_plan_mutation(
        db,
        data.user_id,
        "quota_plan.updated",
        plan.id,
        json!({
            "code": plan.code,
            "made_default": body.is_default == Some(true) && !current.is_default,
            "removed_limits": removed.iter().map(|d| d.as_str()).collect::<Vec<_>>(),
        }),
    )
    .await;

    Ok(ok_response(json!({
        "plan": {
            "id": plan.id,
            "code": plan.code,
            "name": plan.name,
            "enabled": plan.enabled,
            "is_default": plan.is_default || body.is_default == Some(true),
        },
    })))
}
