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
struct RenameRequest {
    name: String,
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

async fn rename(
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

async fn remove(
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
    let references = transaction
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"
                SELECT EXISTS (
                    SELECT 1 FROM nodes
                    WHERE region = $1
                        OR desired_config #>> '{node,region}' = $1
                        OR effective_config #>> '{node,region}' = $1
                    UNION ALL SELECT 1 FROM deployments WHERE region = $1
                    UNION ALL SELECT 1 FROM regional_ingresses WHERE region = $1
                    UNION ALL SELECT 1 FROM host_sources WHERE region = $1
                    UNION ALL SELECT 1 FROM project_host_bindings WHERE region = $1
                ) AS used
            "#,
            [code.into()],
        ))
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::Infrastructure {
            op: OP,
            source: anyhow::anyhow!("Region reference query returned no result"),
        })?;
    let referenced: bool =
        references
            .try_get("", "used")
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
    if referenced || item.code == "default" {
        return Err(AppError::Conflict { op: OP, message: "This region is reserved or still referenced by nodes, configurations, entries, domains, or deployments.".to_owned() });
    }
    region::Entity::delete_by_id(item.code)
        .exec(&transaction)
        .await
        .map_err(|source| {
            if matches!(
                source.sql_err(),
                Some(sea_orm::SqlErr::ForeignKeyConstraintViolation(_))
            ) {
                AppError::Conflict {
                    op: OP,
                    message: "Region is still in use".to_owned(),
                }
            } else {
                AppError::Infrastructure {
                    op: OP,
                    source: source.into(),
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn deleting_a_referenced_region_conflicts_but_storage_failures_do_not() {
        use std::collections::BTreeMap;
        use tower::ServiceExt;
        for (referenced, expected) in [
            (true, axum::http::StatusCode::CONFLICT),
            (false, axum::http::StatusCode::INTERNAL_SERVER_ERROR),
        ] {
            let db = sea_orm::MockDatabase::new(DbBackend::Postgres)
                .append_query_results([vec![region::Model {
                    code: "us-test".to_owned(),
                    name: "Test".to_owned(),
                    created_at: OffsetDateTime::UNIX_EPOCH,
                    updated_at: OffsetDateTime::UNIX_EPOCH,
                }]])
                .append_query_results([vec![BTreeMap::from([(
                    "used",
                    sea_orm::Value::Bool(Some(referenced)),
                )])]])
                .append_exec_errors([sea_orm::DbErr::Custom("storage unavailable".to_owned())])
                .into_connection();
            let state = ControlApiState::new(
                crate::infra::config::ControlApiConfig::default(),
                "unused.toml",
            );
            state.database.set(db).ok().unwrap();
            let response = router()
                .with_state(state)
                .oneshot(
                    axum::http::Request::builder()
                        .method("DELETE")
                        .uri("/regions/us-test")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            let body = axum::body::to_bytes(response.into_body(), 4096)
                .await
                .unwrap();
            assert!(!String::from_utf8_lossy(&body).contains("storage unavailable"));
        }
    }
}
