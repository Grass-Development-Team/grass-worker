use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, Set, sea_query::OnConflict,
};
use subtle::ConstantTimeEq;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{
    NodeStatus, node, node_ingress_status, regional_ingress, regional_ingress_health,
};

pub const HEARTBEAT_STALE_SECONDS: i64 = 90;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CnameGuidance {
    pub record_type: &'static str,
    pub name: String,
    pub target: String,
    pub verification_name: String,
    pub verification_value: String,
    pub region: String,
    pub origin_host_preservation: bool,
}

pub struct CnameGuidanceInput<'a> {
    pub host: &'a str,
    pub region: &'a str,
    pub ingress_hostname: &'a str,
    pub verification_name: &'a str,
    pub verification_value: &'a str,
    pub origin_host_preservation: bool,
}

pub fn cname_guidance(input: CnameGuidanceInput<'_>) -> CnameGuidance {
    CnameGuidance {
        record_type: "CNAME",
        name: input.host.to_owned(),
        target: input.ingress_hostname.to_owned(),
        verification_name: input.verification_name.to_owned(),
        verification_value: input.verification_value.to_owned(),
        region: input.region.to_owned(),
        origin_host_preservation: input.origin_host_preservation,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngressCandidateInput {
    pub node_id: String,
    pub region: String,
    pub base_url: String,
    pub healthy: bool,
    pub priority: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngressCandidate {
    pub node_id: String,
    pub base_url: String,
    pub priority: i32,
}

pub fn healthy_candidates(
    candidates: &[IngressCandidateInput],
    region: &str,
) -> Vec<IngressCandidate> {
    let mut selected = candidates
        .iter()
        .filter(|candidate| candidate.region == region && candidate.healthy)
        .map(|candidate| IngressCandidate {
            node_id: candidate.node_id.clone(),
            base_url: candidate.base_url.clone(),
            priority: candidate.priority,
        })
        .collect::<Vec<_>>();
    selected.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.node_id.cmp(&right.node_id))
    });
    selected
}

pub async fn list<C: ConnectionTrait>(db: &C) -> anyhow::Result<Vec<regional_ingress::Model>> {
    regional_ingress::Entity::find()
        .filter(regional_ingress::Column::DeletedAt.is_null())
        .order_by_asc(regional_ingress::Column::Region)
        .all(db)
        .await
        .map_err(Into::into)
}

pub async fn get_enabled_by_region<C: ConnectionTrait>(
    db: &C,
    region: &str,
) -> anyhow::Result<Option<regional_ingress::Model>> {
    regional_ingress::Entity::find()
        .filter(regional_ingress::Column::Region.eq(region))
        .filter(regional_ingress::Column::Enabled.eq(true))
        .filter(regional_ingress::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(Into::into)
}

pub async fn healthy_serve_nodes<C: ConnectionTrait>(
    db: &C,
    region: &str,
    now: OffsetDateTime,
) -> anyhow::Result<Vec<IngressCandidate>> {
    let ingress = regional_ingress::Entity::find()
        .filter(regional_ingress::Column::Region.eq(region))
        .filter(regional_ingress::Column::Enabled.eq(true))
        .filter(regional_ingress::Column::DeletedAt.is_null())
        .one(db)
        .await?;
    let health_freshness = ingress
        .as_ref()
        .map(|item| health_check_freshness_seconds(item.health_check_interval_seconds))
        .unwrap_or(120);
    let tls_required = ingress.is_some();
    let health = if let Some(ingress) = ingress {
        regional_ingress_health::Entity::find()
            .filter(regional_ingress_health::Column::IngressId.eq(ingress.id))
            .all(db)
            .await?
            .into_iter()
            .map(|item| (item.node_id, item))
            .collect::<std::collections::HashMap<_, _>>()
    } else {
        std::collections::HashMap::new()
    };
    let nodes = node::Entity::find()
        .filter(node::Column::Region.eq(region))
        .filter(node::Column::ServeEnabled.eq(true))
        .filter(node::Column::BaseUrl.is_not_null())
        .filter(node::Column::DeletedAt.is_null())
        .order_by_asc(node::Column::Id)
        .all(db)
        .await?;
    let tls_status = node_ingress_status::Entity::find()
        .filter(node_ingress_status::Column::NodeId.is_in(nodes.iter().map(|n| n.id)))
        .all(db)
        .await?
        .into_iter()
        .map(|s| (s.node_id, s))
        .collect::<std::collections::HashMap<_, _>>();
    let candidates = nodes
        .iter()
        .filter_map(|node| {
            let base_url = node.base_url.as_deref()?;
            Some(IngressCandidateInput {
                node_id: node.id.to_string(),
                region: node.region.clone(),
                base_url: base_url.to_owned(),
                healthy: matches!(node.status, NodeStatus::Active)
                    && (!tls_required
                        || tls_status.get(&node.id).is_some_and(|s| {
                            s.tls_ready && (now - s.checked_at).whole_seconds() <= 30
                        }))
                    && node.last_heartbeat_at.is_some_and(|heartbeat| {
                        (now - heartbeat).whole_seconds() <= HEARTBEAT_STALE_SECONDS
                    })
                    && health.get(&node.id).is_some_and(|item| {
                        item.status == "healthy"
                            && item.checked_at.is_some_and(|checked| {
                                (now - checked).whole_seconds() <= i64::from(health_freshness)
                            })
                    }),
                priority: 0,
            })
        })
        .collect::<Vec<_>>();
    Ok(healthy_candidates(&candidates, region))
}

fn health_check_freshness_seconds(interval_seconds: i32) -> i32 {
    interval_seconds.saturating_mul(3).max(60)
}

pub fn health_check_due(
    last_checked: Option<OffsetDateTime>,
    interval_seconds: i32,
    now: OffsetDateTime,
) -> bool {
    last_checked
        .is_none_or(|checked| (now - checked).whole_seconds() >= i64::from(interval_seconds.max(5)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub status: &'static str,
    pub latency_ms: Option<i32>,
    pub error: Option<String>,
}

pub async fn probe_node(
    client: &reqwest::Client,
    base_url: &str,
    hostname: &str,
    path: &str,
) -> ProbeResult {
    let started = std::time::Instant::now();
    let url = match url::Url::parse(base_url).and_then(|base| base.join(path)) {
        Ok(url) if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() => url,
        _ => {
            return ProbeResult {
                status: "unhealthy",
                latency_ms: None,
                error: Some("invalid node base URL".to_owned()),
            };
        }
    };
    let response = client
        .get(url)
        .header(reqwest::header::HOST, hostname)
        .send()
        .await;
    let latency_ms = i32::try_from(started.elapsed().as_millis()).ok();
    match response {
        Ok(response) if response.status().is_success() => ProbeResult {
            status: "healthy",
            latency_ms,
            error: None,
        },
        Ok(response) => ProbeResult {
            status: "unhealthy",
            latency_ms,
            error: Some(format!("health endpoint returned {}", response.status())),
        },
        Err(error) => ProbeResult {
            status: "unhealthy",
            latency_ms,
            error: Some(error.without_url().to_string()),
        },
    }
}

pub async fn probe_regional_ingress(
    db: &sea_orm::DatabaseConnection,
    ingress: &regional_ingress::Model,
    now: OffsetDateTime,
) -> anyhow::Result<()> {
    let last_checked = regional_ingress_health::Entity::find()
        .filter(regional_ingress_health::Column::IngressId.eq(ingress.id))
        .order_by_desc(regional_ingress_health::Column::CheckedAt)
        .one(db)
        .await?
        .and_then(|item| item.checked_at);
    if !health_check_due(last_checked, ingress.health_check_interval_seconds, now) {
        return Ok(());
    }
    if ingress
        .dns_checked_at
        .is_none_or(|at| now - at >= time::Duration::seconds(60))
    {
        let result = super::domain_dns::Resolver::new()?
            .addresses(&ingress.hostname)
            .await;
        let mut active: regional_ingress::ActiveModel = ingress.clone().into();
        active.dns_checked_at = Set(Some(now));
        match result {
            Ok(addresses) if !addresses.is_empty() => {
                active.dns_status = Set("resolved".to_owned());
                active.dns_error = Set(None);
            }
            Ok(_) => {
                active.dns_status = Set("unresolved".to_owned());
                active.dns_error = Set(Some(
                    "Add DNS address records for this CNAME target at your DNS provider."
                        .to_owned(),
                ));
            }
            Err(_) => {
                active.dns_status = Set("error".to_owned());
                active.dns_error = Set(Some(
                    "Public DNS could not be checked; automatic retry is scheduled.".to_owned(),
                ));
            }
        }
        // A concurrent hostname edit must not receive a result for the old hostname.
        regional_ingress::Entity::update(active)
            .validate()?
            .filter(regional_ingress::Column::Hostname.eq(&ingress.hostname))
            .exec(db)
            .await?;
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(3))
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let nodes = node::Entity::find()
        .filter(node::Column::Region.eq(&ingress.region))
        .filter(node::Column::ServeEnabled.eq(true))
        .filter(node::Column::BaseUrl.is_not_null())
        .filter(node::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    for node in nodes {
        let result = probe_node(
            &client,
            node.base_url.as_deref().unwrap_or_default(),
            &ingress.hostname,
            &ingress.health_check_path,
        )
        .await;
        let active = regional_ingress_health::ActiveModel {
            ingress_id: Set(ingress.id),
            node_id: Set(node.id),
            status: Set(result.status.to_owned()),
            checked_at: Set(Some(now)),
            latency_ms: Set(result.latency_ms),
            error: Set(result.error),
        };
        regional_ingress_health::Entity::insert(active)
            .on_conflict(
                OnConflict::columns([
                    regional_ingress_health::Column::IngressId,
                    regional_ingress_health::Column::NodeId,
                ])
                .update_columns([
                    regional_ingress_health::Column::Status,
                    regional_ingress_health::Column::CheckedAt,
                    regional_ingress_health::Column::LatencyMs,
                    regional_ingress_health::Column::Error,
                ])
                .to_owned(),
            )
            .exec(db)
            .await?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsVerification {
    Verified,
    Missing,
    Mismatch,
}

pub async fn verify_dns_txt_at(
    client: &reqwest::Client,
    endpoint: &str,
    host: &str,
    expected: &str,
) -> anyhow::Result<DnsVerification> {
    let name = format!("_grass.{}", host.trim_end_matches('.'));
    verify_dns_record_at(client, endpoint, &name, "TXT", expected).await
}

pub async fn verify_dns_record_at(
    client: &reqwest::Client,
    endpoint: &str,
    name: &str,
    record_type: &str,
    expected: &str,
) -> anyhow::Result<DnsVerification> {
    let response = client
        .get(endpoint)
        .query(&[("name", name), ("type", record_type)])
        .header("accept", "application/dns-json")
        .send()
        .await?;
    if !response.status().is_success() {
        anyhow::bail!("DNS TXT query returned {}", response.status());
    }
    let body: serde_json::Value = response.json().await?;
    if body
        .get("Status")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|status| status != 0 && status != 3)
    {
        anyhow::bail!("DNS resolver returned an unsuccessful status");
    }
    let answers = body
        .get("Answer")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut found = false;
    for answer in answers {
        let wanted = if record_type == "CNAME" { 5 } else { 16 };
        if answer.get("type").and_then(serde_json::Value::as_u64) != Some(wanted) {
            continue;
        }
        let Some(data) = answer.get("data").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let text = if record_type == "CNAME" {
            data.trim().trim_end_matches('.').to_ascii_lowercase()
        } else {
            data.trim().trim_matches('"').replace("\" \"", "")
        };
        let value = text.as_str();
        if value.as_bytes().ct_eq(expected.as_bytes()).into() {
            return Ok(DnsVerification::Verified);
        }
        found = true;
    }
    Ok(if found {
        DnsVerification::Mismatch
    } else {
        DnsVerification::Missing
    })
}

pub fn dns_verification_token(secret_key: &str, binding_id: Uuid, host: &str) -> String {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret_key.as_bytes());
    let message = format!("grass-domain-v1:{binding_id}:{host}");
    format!(
        "grass-{}",
        hex::encode(ring::hmac::sign(&key, message.as_bytes()).as_ref())
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{CnameGuidanceInput, IngressCandidateInput, cname_guidance, healthy_candidates};
    use axum::{Json, Router, routing::get};
    pub(crate) fn node_fixture() -> super::node::Model {
        let now = time::OffsetDateTime::now_utc();
        super::node::Model {
            id: uuid::Uuid::now_v7(),
            name: "entry".to_owned(),
            region: "eu".to_owned(),
            token_hash: "hash".to_owned(),
            status: super::NodeStatus::Active,
            build_enabled: false,
            serve_enabled: true,
            build_concurrency: 1,
            base_url: Some("http://entry.example.org".to_owned()),
            work_root: None,
            capacity_cpu_millicores: 2000,
            capacity_memory_mb: 2048,
            capacity_disk_mb: 10000,
            max_deployments: 10,
            metadata: serde_json::json!({}),
            last_heartbeat_at: Some(now),
            desired_config: None,
            desired_config_revision: 0,
            effective_config: None,
            effective_config_revision: 0,
            config_sync_status: crate::infra::database::entity::NodeConfigSyncStatus::Applied,
            config_sync_error: None,
            node_token_configured: true,
            config_updated_at: None,
            config_applied_at: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn tls_ingress_excludes_http_only_and_stale_tls_nodes() {
        use super::*;
        let now = OffsetDateTime::now_utc();
        let ingress = crate::domain::certificates::tests::ingress_fixture();
        let ready = node_fixture();
        let http_only = node_fixture();
        let stale = node_fixture();
        let nodes = vec![ready.clone(), http_only.clone(), stale.clone()];
        let health = nodes
            .iter()
            .map(|node| regional_ingress_health::Model {
                ingress_id: ingress.id,
                node_id: node.id,
                status: "healthy".to_owned(),
                checked_at: Some(now),
                latency_ms: Some(1),
                error: None,
            })
            .collect::<Vec<_>>();
        let status = nodes
            .iter()
            .map(|node| node_ingress_status::Model {
                node_id: node.id,
                certificates: serde_json::json!([]),
                challenge_revision: String::new(),
                tls_ready: node.id != http_only.id,
                checked_at: if node.id == stale.id {
                    now - time::Duration::seconds(60)
                } else {
                    now
                },
            })
            .collect::<Vec<_>>();
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([vec![ingress]])
            .append_query_results([health])
            .append_query_results([nodes])
            .append_query_results([status])
            .into_connection();
        let selected = healthy_serve_nodes(&db, "eu", now).await.unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].node_id, ready.id.to_string());
    }

    #[tokio::test]
    async fn dns_txt_verification_accepts_matching_answer_and_rejects_missing() {
        let app = Router::new().route(
            "/dns-query",
            get(|| async {
                Json(serde_json::json!({
                    "Answer": [{"type": 16, "data": "\"expected-token\""}]
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/dns-query", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        assert_eq!(
            super::verify_dns_txt_at(&client, &endpoint, "app.example.com", "expected-token")
                .await
                .unwrap(),
            super::DnsVerification::Verified
        );
        assert_eq!(
            super::verify_dns_txt_at(&client, &endpoint, "app.example.com", "other-token")
                .await
                .unwrap(),
            super::DnsVerification::Mismatch
        );
        server.abort();
    }

    #[test]
    fn cname_guidance_includes_cname_and_txt_ownership_records() {
        let guidance = cname_guidance(CnameGuidanceInput {
            host: "app.example.com",
            region: "eu-west",
            ingress_hostname: "eu-west.edge.example.net",
            verification_name: "_grass.app.example.com",
            verification_value: "grass-token",
            origin_host_preservation: true,
        });

        assert_eq!(guidance.record_type, "CNAME");
        assert_eq!(guidance.name, "app.example.com");
        assert_eq!(guidance.target, "eu-west.edge.example.net");
        assert_eq!(guidance.verification_name, "_grass.app.example.com");
        assert_eq!(guidance.verification_value, "grass-token");
        assert!(guidance.origin_host_preservation);
    }

    #[test]
    fn healthy_candidates_are_limited_to_region_and_sorted_by_priority() {
        let candidates = vec![
            IngressCandidateInput {
                node_id: "node-2".to_owned(),
                region: "eu-west".to_owned(),
                base_url: "https://node-2.example.net".to_owned(),
                healthy: true,
                priority: 20,
            },
            IngressCandidateInput {
                node_id: "node-1".to_owned(),
                region: "eu-west".to_owned(),
                base_url: "https://node-1.example.net".to_owned(),
                healthy: true,
                priority: 10,
            },
            IngressCandidateInput {
                node_id: "node-3".to_owned(),
                region: "us-east".to_owned(),
                base_url: "https://node-3.example.net".to_owned(),
                healthy: true,
                priority: 1,
            },
            IngressCandidateInput {
                node_id: "node-4".to_owned(),
                region: "eu-west".to_owned(),
                base_url: "https://node-4.example.net".to_owned(),
                healthy: false,
                priority: 0,
            },
        ];

        let selected = healthy_candidates(&candidates, "eu-west");

        assert_eq!(
            selected
                .iter()
                .map(|candidate| candidate.node_id.as_str())
                .collect::<Vec<_>>(),
            vec!["node-1", "node-2"]
        );
    }
}
