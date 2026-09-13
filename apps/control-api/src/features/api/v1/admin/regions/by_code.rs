use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, DbBackend, EntityTrait, QuerySelect, Set, Statement,
    TransactionTrait,
};
use serde::Deserialize;
use time::OffsetDateTime;

use crate::{
    infra::{
        database::entity::region,
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/regions/{code}",
        axum::routing::delete(remove).patch(rename),
    )
}

#[derive(Deserialize)]
pub struct RenameRequest {
    pub name: String,
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

pub async fn rename(
    State(state): State<ControlApiState>,
    Path(code): Path<String>,
    Json(body): Json<RenameRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regions.rename";
    let db = crate::infra::http::database(&state, OP)?;
    let item = region::Entity::find_by_id(&code)
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "Region not found".to_owned(),
        })?;
    let mut active: region::ActiveModel = item.into();
    active.name = Set(name(&body.name, OP)?);
    active.updated_at = Set(OffsetDateTime::now_utc());
    active
        .update(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(RenameResponse { ok: true }))
}

pub async fn remove(
    State(state): State<ControlApiState>,
    Path(code): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regions.remove";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let item = region::Entity::find_by_id(&code)
        .lock_exclusive()
        .one(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "Region not found".to_owned(),
        })?;
    let referenced = transaction.query_one_raw(Statement::from_sql_and_values(DbBackend::Postgres, r#"
SELECT EXISTS(SELECT 1 FROM nodes WHERE region = $1 OR desired_config #>> '{node,region}' = $1 OR effective_config #>> '{node,region}' = $1
UNION ALL SELECT 1 FROM deployments WHERE region = $1
UNION ALL SELECT 1 FROM regional_ingresses WHERE region = $1
UNION ALL SELECT 1 FROM host_sources WHERE region = $1
UNION ALL SELECT 1 FROM project_host_bindings WHERE region = $1) AS used
"#, [code.into()])).await.map_err(|source| AppError::Infrastructure { op: OP, source: source.into() })?.and_then(|r| r.try_get::<bool>("", "used").ok()).unwrap_or(true);
    if referenced || item.code == "default" {
        return Err(AppError::Conflict { op: OP, message: "This region is reserved or still referenced by nodes, configurations, entries, domains, or deployments.".to_owned() });
    }
    region::Entity::delete_by_id(item.code)
        .exec(&transaction)
        .await
        .map_err(|source| AppError::Conflict {
            op: OP,
            message: if source.sql_err().is_some() {
                "Region is still in use".to_owned()
            } else {
                "Region could not be removed".to_owned()
            },
        })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(RemoveResponse { ok: true }))
}

#[derive(serde::Serialize)]
struct RenameResponse {
    ok: bool,
}

#[derive(serde::Serialize)]
struct RemoveResponse {
    ok: bool,
}
