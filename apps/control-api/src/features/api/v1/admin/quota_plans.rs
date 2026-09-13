pub(crate) mod by_plan_id;

use axum::{Json, extract::State, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::quotas::{self, CreatePlanParams, QuotaDimension},
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, QuotaPeriod, quota_limit, quota_plan},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/quota-plans", axum::routing::get(list).post(create))
        .merge(by_plan_id::router())
}

#[derive(Deserialize)]
struct QuotaLimitInput {
    dimension: String,
    /// `null` removes the limit row (unlimited).
    limit_value: Option<i64>,
}

#[derive(Deserialize)]
struct CreateQuotaPlanRequest {
    code: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    limits: Vec<QuotaLimitInput>,
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
    db: &impl audits::AuditConnection,
    actor: Uuid,
    action: &str,
    plan_id: Uuid,
    metadata: serde_json::Value,
) -> anyhow::Result<()> {
    audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(actor),
            actor_node_id: None,
            team_id: None,
            action: action.to_owned(),
            target_type: "quota_plan".to_owned(),
            target_id: Some(plan_id),
            result: AuditEventResult::Success,
            reason: None,
            metadata,
        },
    )
    .await
}

/// GET /api/v1/admin/quota-plans
async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.quota_plans.list";
    let db = crate::infra::http::database(&state, OP)?;
    let plans = quotas::list_plans(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(ListResponse {
        plans: plans
            .into_iter()
            .map(|(plan, limits)| plan_view(plan, limits))
            .collect::<Vec<_>>(),
    }))
}

/// POST /api/v1/admin/quota-plans
async fn create(
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

    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
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

    audit_plan_mutation(
        &transaction,
        data.user_id,
        "quota_plan.created",
        plan.id,
        json!({ "code": plan.code }),
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
        plan: CreatePlanResponse {
            id: plan.id,
            code: plan.code.clone(),
            name: plan.name.clone(),
        },
    }))
}

#[derive(serde::Serialize)]
struct ListResponse {
    plans: Vec<QuotaPlanView>,
}

#[derive(serde::Serialize)]
struct CreatePlanResponse {
    id: uuid::Uuid,
    code: String,
    name: String,
}

#[derive(serde::Serialize)]
struct CreateResponse {
    plan: CreatePlanResponse,
}
