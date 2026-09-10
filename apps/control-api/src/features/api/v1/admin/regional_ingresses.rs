use crate::{
    domain::{ingress, regions},
    infra::{
        database::entity::{
            node, node_ingress_status, project_host_binding, regional_ingress,
            regional_ingress_health,
        },
        error::{AppError, ok_response},
        http::timestamps::ts,
    },
    state::ControlApiState,
};
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
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

async fn detailed_view(
    db: &sea_orm::DatabaseConnection,
    item: &regional_ingress::Model,
) -> anyhow::Result<serde_json::Value> {
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
        statuses.push(json!({"node_id":node.id,"tls_ready":status.as_ref().is_some_and(|s|s.tls_ready && OffsetDateTime::now_utc()-s.checked_at < time::Duration::seconds(30)),"checked_at":ts(status.as_ref().map(|s|s.checked_at)),"health_status":health.as_ref().map(|h|h.status.as_str()).unwrap_or("unknown")}));
    }
    let healthy = ingress::healthy_serve_nodes(db, &item.region, OffsetDateTime::now_utc()).await?;
    Ok(
        json!({"id":item.id,"region":item.region,"hostname":item.hostname,"enabled":item.enabled,"health_check_path":item.health_check_path,"health_check_interval_seconds":item.health_check_interval_seconds,"origin_host_preservation":true,"dns_status":item.dns_status,"dns_error":item.dns_error,"dns_checked_at":ts(item.dns_checked_at),"healthy_nodes":healthy.iter().map(|n|json!({"node_id":n.node_id,"base_url":n.base_url,"priority":n.priority})).collect::<Vec<_>>(),"node_statuses":statuses,"created_at":ts(item.created_at),"updated_at":ts(item.updated_at)}),
    )
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

fn default_enabled() -> bool {
    true
}
fn default_path() -> String {
    "/_grass/health".to_owned()
}
fn default_interval() -> i32 {
    30
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    pub region: String,
    pub hostname: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_path")]
    pub health_check_path: String,
    #[serde(default = "default_interval")]
    pub health_check_interval_seconds: i32,
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
pub async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regional_ingresses.list";
    let db = super::database(&state, OP)?;
    let entries = ingress::list(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let mut views = Vec::new();
    for item in entries {
        views.push(
            detailed_view(db, &item)
                .await
                .map_err(|source| AppError::Infrastructure { op: OP, source })?,
        );
    }
    Ok(ok_response(json!({"regional_ingresses":views})))
}
pub async fn create(
    State(state): State<ControlApiState>,
    Json(body): Json<CreateRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regional_ingresses.create";
    let db = super::database(&state, OP)?;
    let region =
        grass_validator::normalize_region(&body.region).map_err(|e| AppError::Validation {
            op: OP,
            message: e.to_string(),
        })?;
    regions::require(db, &region, OP).await?;
    let hostname =
        grass_validator::normalize_host(&body.hostname).map_err(|e| AppError::Validation {
            op: OP,
            message: e.to_string(),
        })?;
    let path = validate_health_check(
        &body.health_check_path,
        body.health_check_interval_seconds,
        OP,
    )?;
    let now = OffsetDateTime::now_utc();
    let item = regional_ingress::ActiveModel {
        id: Set(Uuid::now_v7()),
        region: Set(region),
        hostname: Set(hostname),
        enabled: Set(body.enabled),
        health_check_path: Set(path),
        health_check_interval_seconds: Set(body.health_check_interval_seconds),
        origin_host_preservation: Set(true),
        dns_status: Set("pending".to_owned()),
        dns_checked_at: Set(None),
        dns_error: Set(None),
        deleted_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .map_err(|source| write_error(source, OP))?;
    Ok(ok_response(
        json!({"regional_ingress":detailed_view(db,&item).await.map_err(|source| AppError::Infrastructure { op: OP, source })?}),
    ))
}
pub async fn update(
    State(state): State<ControlApiState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regional_ingresses.update";
    let db = super::database(&state, OP)?;
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
    Ok(ok_response(
        json!({"regional_ingress":detailed_view(db,&item).await.map_err(|source| AppError::Infrastructure { op: OP, source })?}),
    ))
}
pub async fn remove(
    State(state): State<ControlApiState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regional_ingresses.remove";
    let db = super::database(&state, OP)?;
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
    Ok(ok_response(json!({"ok":true})))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manual_entry_needs_only_an_existing_region_and_hostname() {
        let body: CreateRequest =
            serde_json::from_value(json!({"region":"hk_1","hostname":"hk.entry.example.com"}))
                .unwrap();
        assert_eq!(body.health_check_path, "/_grass/health");
        assert!(serde_json::from_value::<CreateRequest>(json!({"region":"hk_1","hostname":"hk.entry.example.com","dns_challenge_provider":"cloudflare"})).is_err());
        assert!(serde_json::from_value::<UpdateRequest>(json!({"tls_enabled":false})).is_err());
    }
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
