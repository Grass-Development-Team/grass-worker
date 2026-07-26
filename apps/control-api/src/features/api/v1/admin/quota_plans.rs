use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::TransactionTrait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::quotas::{self, CreatePlanParams, QuotaDimension, UpdatePlanParams},
    infra::{
        database::entity::{QuotaPeriod, quota_limit, quota_plan},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct QuotaLimitInput {
    pub dimension: String,
    pub limit_value: i64,
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

fn parse_limits(
    limits: Vec<QuotaLimitInput>,
    op: &'static str,
) -> Result<Vec<(QuotaDimension, i64)>, AppError> {
    limits
        .into_iter()
        .map(|limit| {
            QuotaDimension::parse(&limit.dimension)
                .map(|dimension| (dimension, limit.limit_value))
                .ok_or_else(|| AppError::Validation {
                    op,
                    message: format!("unknown quota dimension: {}", limit.dimension),
                })
        })
        .collect()
}

/// GET /api/v1/admin/quota-plans
pub async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.quota_plans.list";
    let db = database(&state, OP)?;
    let plans = quotas::list_plans(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(serde_json::json!({
        "plans": plans
            .into_iter()
            .map(|(plan, limits)| plan_view(plan, limits))
            .collect::<Vec<_>>(),
    })))
}

/// POST /api/v1/admin/quota-plans
pub async fn create(
    State(state): State<ControlApiState>,
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
    let limits = parse_limits(body.limits, OP)?;

    let db = database(&state, OP)?;
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

    Ok(ok_response(serde_json::json!({
        "plan": { "id": plan.id, "code": plan.code, "name": plan.name },
    })))
}

/// PATCH /api/v1/admin/quota-plans/{plan_id}
pub async fn update(
    State(state): State<ControlApiState>,
    Path(plan_id): Path<Uuid>,
    Json(body): Json<UpdateQuotaPlanRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.quota_plans.update";
    let limits = parse_limits(body.limits, OP)?;

    let db = database(&state, OP)?;
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
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(serde_json::json!({
        "plan": { "id": plan.id, "code": plan.code, "name": plan.name, "enabled": plan.enabled },
    })))
}

fn database<'a>(
    state: &'a ControlApiState,
    op: &'static str,
) -> Result<&'a sea_orm::DatabaseConnection, AppError> {
    state.try_database().ok_or_else(|| AppError::Internal {
        op,
        message: "database not available".to_owned(),
    })
}
