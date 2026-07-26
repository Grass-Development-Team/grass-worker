use axum::{extract::State, response::IntoResponse};
use serde::Serialize;

use crate::{
    domain::{
        quotas::{self, QuotaDimension},
        teams,
    },
    infra::{
        database::entity::QuotaPeriod,
        error::{AppError, ok_response},
        http::extractors::TeamRole,
    },
    state::ControlApiState,
};

#[derive(Serialize)]
struct QuotaPlanView {
    id: uuid::Uuid,
    code: String,
    name: String,
    source: &'static str,
}

#[derive(Serialize)]
struct QuotaLimitView {
    dimension: &'static str,
    limit: Option<i64>,
    period: &'static str,
}

#[derive(Serialize)]
struct QuotaUsageView {
    dimension: &'static str,
    limit: Option<i64>,
    used: i64,
    period: &'static str,
}

fn period_value(period: &QuotaPeriod) -> &'static str {
    match period {
        QuotaPeriod::None => "none",
        QuotaPeriod::Monthly => "monthly",
    }
}

async fn resolve(
    state: &ControlApiState,
    role: &TeamRole,
    op: &'static str,
) -> Result<(quotas::ResolvedQuota, &'static str), AppError> {
    let db = super::database(state, op)?;
    let team = teams::get_by_id(db, role.team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "team not found".to_owned(),
        })?;
    let resolved = quotas::resolve_team_quota(db, &team)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    let source = resolved.source.as_str();
    Ok((resolved, source))
}

/// GET /api/v1/teams/{team_id}/quota
pub async fn plan(
    State(state): State<ControlApiState>,
    role: TeamRole,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "teams.quota.plan";
    let (resolved, source) = resolve(&state, &role, OP).await?;

    let limits = QuotaDimension::ALL
        .iter()
        .map(|dimension| QuotaLimitView {
            dimension: dimension.as_str(),
            limit: resolved.limit_for(*dimension),
            period: period_value(&dimension.period()),
        })
        .collect::<Vec<_>>();

    Ok(ok_response(serde_json::json!({
        "plan": QuotaPlanView {
            id: resolved.plan.id,
            code: resolved.plan.code.clone(),
            name: resolved.plan.name.clone(),
            source,
        },
        "limits": limits,
    })))
}

/// GET /api/v1/teams/{team_id}/quota/usage
pub async fn usage(
    State(state): State<ControlApiState>,
    role: TeamRole,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "teams.quota.usage";
    let db = super::database(&state, OP)?;
    let (resolved, source) = resolve(&state, &role, OP).await?;

    let mut usage = Vec::with_capacity(QuotaDimension::ALL.len());
    for dimension in QuotaDimension::ALL {
        if !dimension.is_counted() {
            continue;
        }
        let used = quotas::effective_usage(db, role.team_id, *dimension)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        usage.push(QuotaUsageView {
            dimension: dimension.as_str(),
            limit: resolved.limit_for(*dimension),
            used,
            period: period_value(&dimension.period()),
        });
    }

    Ok(ok_response(serde_json::json!({
        "plan": QuotaPlanView {
            id: resolved.plan.id,
            code: resolved.plan.code.clone(),
            name: resolved.plan.name.clone(),
            source,
        },
        "usage": usage,
    })))
}
