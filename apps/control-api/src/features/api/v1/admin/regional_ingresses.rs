pub(crate) mod by_ingress_id;

use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::Deserialize;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::{ingress, regions},
    infra::{
        database::entity::{node, node_ingress_status, regional_ingress, regional_ingress_health},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/regional-ingresses", axum::routing::get(list).post(create))
        .merge(by_ingress_id::router())
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
    let db = crate::infra::http::database(&state, OP)?;
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
    Ok(ok_response(ListResponse {
        regional_ingresses: views,
    }))
}

pub async fn create(
    State(state): State<ControlApiState>,
    Json(body): Json<CreateRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regional_ingresses.create";
    let db = crate::infra::http::database(&state, OP)?;
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
        dns_status: Set(ingress::IngressDnsStatus::Pending.as_str().to_owned()),
        dns_checked_at: Set(None),
        dns_error: Set(None),
        deleted_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .map_err(|source| write_error(source, OP))?;
    Ok(ok_response(CreateResponse {
        regional_ingress: detailed_view(db, &item)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?,
    }))
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
struct ListResponse {
    regional_ingresses: Vec<RegionalIngressResponse>,
}

#[derive(serde::Serialize)]
struct CreateResponse {
    regional_ingress: RegionalIngressResponse,
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    #[derive(serde::Serialize)]
    pub struct UpdateRequest {
        pub hostname: Option<String>,
        pub enabled: Option<bool>,
        pub health_check_path: Option<String>,
        pub health_check_interval_seconds: Option<i32>,
    }

    #[test]
    fn manual_entry_needs_only_an_existing_region_and_hostname() {
        let body: CreateRequest =
            serde_json::from_value(json!({"region":"hk_1","hostname":"hk.entry.example.com"}))
                .unwrap();
        assert_eq!(body.health_check_path, "/_grass/health");
        assert!(serde_json::from_value::<CreateRequest>(json!({"region":"hk_1","hostname":"hk.entry.example.com","dns_challenge_provider":"cloudflare"})).is_err());
        assert!(serde_json::from_value::<UpdateRequest>(json!({"tls_enabled":false})).is_err());
    }
}
