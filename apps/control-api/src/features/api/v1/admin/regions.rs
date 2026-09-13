pub(crate) mod by_code;

use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::{ActiveModelTrait, Set};
use serde::Deserialize;
use time::OffsetDateTime;

use crate::{
    domain::regions,
    infra::{
        database::entity::region,
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/regions", axum::routing::get(list).post(create))
        .merge(by_code::router())
}

#[derive(Deserialize)]
struct RegionRequest {
    code: String,
    name: Option<String>,
}

fn name(value: &str, op: &'static str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 128 {
        return Err(AppError::Validation {
            op,
            message: "Region name must contain 1–128 characters".to_owned(),
        });
    }
    Ok(value.to_owned())
}

async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regions.list";
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

async fn create(
    State(state): State<ControlApiState>,
    Json(body): Json<RegionRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regions.create";
    let code = grass_validator::normalize_region(&body.code).map_err(|e| AppError::Validation {
        op: OP,
        message: e.to_string(),
    })?;
    let name = name(body.name.as_deref().unwrap_or(&code), OP)?;
    let now = OffsetDateTime::now_utc();
    let region = region::ActiveModel {
        code: Set(code),
        name: Set(name),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(crate::infra::http::database(&state, OP)?)
    .await
    .map_err(|source| {
        if matches!(
            source.sql_err(),
            Some(sea_orm::SqlErr::UniqueConstraintViolation(_))
        ) {
            AppError::Conflict {
                op: OP,
                message: "This region already exists. Select it from the list.".to_owned(),
            }
        } else {
            AppError::Infrastructure {
                op: OP,
                source: source.into(),
            }
        }
    })?;
    Ok(ok_response(CreateResponse {
        region: CreateRegionResponse {
            code: region.code.clone(),
            name: region.name.clone(),
        },
    }))
}

#[derive(serde::Serialize)]
struct CreateRegionResponse {
    code: String,
    name: String,
}

#[derive(serde::Serialize)]
struct CreateResponse {
    region: CreateRegionResponse,
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
