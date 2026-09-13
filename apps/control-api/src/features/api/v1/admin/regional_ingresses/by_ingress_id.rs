use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect, Set,
    TransactionTrait,
};
use serde::Deserialize;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::ingress,
    infra::{
        database::entity::{
            node, node_ingress_status, project_host_binding, regional_ingress,
            regional_ingress_health,
        },
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/regional-ingresses/{ingress_id}",
        axum::routing::delete(remove).patch(update),
    )
}

async fn detailed_view(
    db: &sea_orm::DatabaseConnection,
    item: &regional_ingress::Model,
) -> anyhow::Result<RegionalIngressResponse> {
    let nodes = node::Entity::find()
        .filter(node::Column::Region.eq(&item.region))
        .filter(node::Column::ServeEnabled.eq(true))
        .filter(node::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    let mut statuses = Vec::new();
    for node in nodes {
        let status = node_ingress_status::Entity::find_by_id(node.id)
            .one(db)
            .await?;
        let health = regional_ingress_health::Entity::find_by_id((item.id, node.id))
            .one(db)
            .await?;
        statuses.push(NodeStatusResponse {
            node_id: node.id,
            tls_ready: status.as_ref().is_some_and(|s| {
                s.tls_ready
                    && OffsetDateTime::now_utc() - s.checked_at < time::Duration::seconds(30)
            }),
            checked_at: status.as_ref().map(|s| s.checked_at),
            health_status: health
                .as_ref()
                .map(|h| h.status.clone())
                .unwrap_or_else(|| "unknown".to_owned()),
        });
    }
    let healthy = ingress::healthy_serve_nodes(db, &item.region, OffsetDateTime::now_utc()).await?;
    Ok(RegionalIngressResponse {
        id: item.id,
        region: item.region.clone(),
        hostname: item.hostname.clone(),
        enabled: item.enabled,
        health_check_path: item.health_check_path.clone(),
        health_check_interval_seconds: item.health_check_interval_seconds,
        origin_host_preservation: true,
        dns_status: item.dns_status.clone(),
        dns_error: item.dns_error.clone(),
        dns_checked_at: item.dns_checked_at,
        healthy_nodes: healthy
            .iter()
            .map(|node| HealthyNodeResponse {
                node_id: node.node_id.clone(),
                base_url: node.base_url.clone(),
                priority: node.priority,
            })
            .collect(),
        node_statuses: statuses,
        created_at: item.created_at,
        updated_at: item.updated_at,
    })
}

fn validate_health_check(path: &str, interval: i32, op: &'static str) -> Result<String, AppError> {
    let path = path.trim();
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains(['?', '#', '\\'])
        || path.len() > 512
    {
        return Err(AppError::Validation {
            op,
            message: "health_check_path must start with / and be at most 512 bytes".to_owned(),
        });
    }
    if !(5..=3600).contains(&interval) {
        return Err(AppError::Validation {
            op,
            message: "health_check_interval_seconds must be between 5 and 3600".to_owned(),
        });
    }
    Ok(path.to_owned())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequest {
    pub hostname: Option<String>,
    pub enabled: Option<bool>,
    pub health_check_path: Option<String>,
    pub health_check_interval_seconds: Option<i32>,
}

fn write_error(source: sea_orm::DbErr, op: &'static str) -> AppError {
    if matches!(
        source.sql_err(),
        Some(sea_orm::SqlErr::UniqueConstraintViolation(_))
    ) {
        AppError::Conflict {
            op,
            message: "This region or CNAME target already has an entry.".to_owned(),
        }
    } else {
        AppError::Infrastructure {
            op,
            source: source.into(),
        }
    }
}

pub async fn update(
    State(state): State<ControlApiState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regional_ingresses.update";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = db.begin().await.map_err(|source| write_error(source, OP))?;
    let item = regional_ingress::Entity::find_by_id(id)
        .filter(regional_ingress::Column::DeletedAt.is_null())
        .lock_exclusive()
        .one(&transaction)
        .await
        .map_err(|source| write_error(source, OP))?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "Regional entry not found".to_owned(),
        })?;
    let path = validate_health_check(
        body.health_check_path
            .as_deref()
            .unwrap_or(&item.health_check_path),
        body.health_check_interval_seconds
            .unwrap_or(item.health_check_interval_seconds),
        OP,
    )?;
    let mut active: regional_ingress::ActiveModel = item.clone().into();
    if let Some(hostname) = body.hostname {
        active.hostname =
            Set(
                grass_validator::normalize_host(&hostname).map_err(|e| AppError::Validation {
                    op: OP,
                    message: e.to_string(),
                })?,
            );
        active.dns_status = Set("pending".to_owned());
        active.dns_checked_at = Set(None);
        active.dns_error = Set(None);
    }
    if let Some(enabled) = body.enabled {
        active.enabled = Set(enabled);
    }
    active.health_check_path = Set(path);
    active.health_check_interval_seconds = Set(body
        .health_check_interval_seconds
        .unwrap_or(item.health_check_interval_seconds));
    active.updated_at = Set(OffsetDateTime::now_utc());
    let item = active
        .update(&transaction)
        .await
        .map_err(|source| write_error(source, OP))?;
    // Invalidate pending connection checks when entry routing changes.
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    transaction.execute_raw(Statement::from_sql_and_values(DbBackend::Postgres, "UPDATE domain_onboarding SET dns_status = 'pending', next_check_at = CURRENT_TIMESTAMP WHERE binding_id IN (SELECT id FROM project_host_bindings WHERE region = $1)", [item.region.clone().into()])).await.map_err(|source| write_error(source, OP))?;
    transaction
        .commit()
        .await
        .map_err(|source| write_error(source, OP))?;
    Ok(ok_response(UpdateResponse {
        regional_ingress: detailed_view(db, &item)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?,
    }))
}

pub async fn remove(
    State(state): State<ControlApiState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regional_ingresses.remove";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = db.begin().await.map_err(|source| write_error(source, OP))?;
    let item = regional_ingress::Entity::find_by_id(id)
        .filter(regional_ingress::Column::DeletedAt.is_null())
        .lock_exclusive()
        .one(&transaction)
        .await
        .map_err(|source| write_error(source, OP))?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "Regional entry not found".to_owned(),
        })?;
    let count = project_host_binding::Entity::find()
        .filter(project_host_binding::Column::Region.eq(&item.region))
        .filter(
            project_host_binding::Column::Kind
                .eq(crate::infra::database::entity::HostBindingKind::Custom),
        )
        .filter(project_host_binding::Column::DeletedAt.is_null())
        .count(&transaction)
        .await
        .map_err(|source| write_error(source, OP))?;
    if count > 0 {
        return Err(AppError::Conflict { op: OP, message: "This regional entry is used by custom domains. Remove their bindings before deleting the entry.".to_owned() });
    }
    let mut active: regional_ingress::ActiveModel = item.into();
    active.deleted_at = Set(Some(OffsetDateTime::now_utc()));
    active.enabled = Set(false);
    active
        .update(&transaction)
        .await
        .map_err(|source| write_error(source, OP))?;
    transaction
        .commit()
        .await
        .map_err(|source| write_error(source, OP))?;
    Ok(ok_response(RemoveResponse { ok: true }))
}

#[derive(serde::Serialize)]
struct RegionalIngressResponse {
    id: Uuid,
    region: String,
    hostname: String,
    enabled: bool,
    health_check_path: String,
    health_check_interval_seconds: i32,
    origin_host_preservation: bool,
    dns_status: String,
    dns_error: Option<String>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    dns_checked_at: Option<time::OffsetDateTime>,
    healthy_nodes: Vec<HealthyNodeResponse>,
    node_statuses: Vec<NodeStatusResponse>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    updated_at: time::OffsetDateTime,
}
#[derive(serde::Serialize)]
struct HealthyNodeResponse {
    node_id: String,
    base_url: String,
    priority: i32,
}
#[derive(serde::Serialize)]
struct NodeStatusResponse {
    node_id: Uuid,
    tls_ready: bool,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    checked_at: Option<time::OffsetDateTime>,
    health_status: String,
}

#[derive(serde::Serialize)]
struct UpdateResponse {
    regional_ingress: RegionalIngressResponse,
}

#[derive(serde::Serialize)]
struct RemoveResponse {
    ok: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_checks_preserve_safe_paths_and_intervals() {
        assert!(validate_health_check("/_grass/health", 30, "test").is_ok());
        for path in [
            "//evil.example/path",
            "https://evil.example",
            "/health?x=1",
            "/health#fragment",
        ] {
            assert!(validate_health_check(path, 30, "test").is_err());
        }
        assert!(validate_health_check("/health", 0, "test").is_err());
    }
}
