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

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/teams/{team_id}/quota/usage", axum::routing::get(usage))
}

#[derive(Serialize)]
struct QuotaPlanView {
    id: uuid::Uuid,
    code: String,
    name: String,
    source: &'static str,
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
    let db = crate::infra::http::database(state, op)?;
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

/// GET /api/v1/teams/{team_id}/quota/usage
async fn usage(
    State(state): State<ControlApiState>,
    role: TeamRole,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "teams.quota.usage";
    let db = crate::infra::http::database(&state, OP)?;
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

    Ok(ok_response(UsageResponse {
        plan: QuotaPlanView {
            id: resolved.plan.id,
            code: resolved.plan.code.clone(),
            name: resolved.plan.name.clone(),
            source,
        },
        usage,
    }))
}

#[derive(serde::Serialize)]
struct UsageResponse {
    plan: QuotaPlanView,
    usage: Vec<QuotaUsageView>,
}
