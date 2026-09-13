use axum::{extract::State, response::IntoResponse};

use crate::{
    domain::regions,
    infra::{
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/regions", axum::routing::get(list))
}

pub async fn list(
    State(state): State<ControlApiState>,
    _session: Session,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "regions.list";
    let regions = regions::available(crate::infra::http::database(&state, OP)?)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(ListResponse {
        regions: regions
            .into_iter()
            .map(|region| RegionResponse {
                code: region.code,
                name: region.name,
                ingress_hostname: region.ingress_hostname,
                ingress_enabled: region.ingress_enabled,
            })
            .collect(),
    }))
}

#[derive(serde::Serialize)]
struct ListResponse {
    regions: Vec<RegionResponse>,
}
#[derive(serde::Serialize)]
struct RegionResponse {
    code: String,
    name: String,
    ingress_hostname: Option<String>,
    ingress_enabled: bool,
}
