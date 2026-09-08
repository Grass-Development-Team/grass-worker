use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{NodeStatus, node, regional_ingress};

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

pub async fn get_by_id<C: ConnectionTrait>(
    db: &C,
    id: Uuid,
) -> anyhow::Result<Option<regional_ingress::Model>> {
    regional_ingress::Entity::find_by_id(id)
        .filter(regional_ingress::Column::DeletedAt.is_null())
        .one(db)
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
    let nodes = node::Entity::find()
        .filter(node::Column::Region.eq(region))
        .filter(node::Column::ServeEnabled.eq(true))
        .filter(node::Column::BaseUrl.is_not_null())
        .filter(node::Column::DeletedAt.is_null())
        .order_by_asc(node::Column::Id)
        .all(db)
        .await?;
    let candidates = nodes
        .iter()
        .filter_map(|node| {
            let base_url = node.base_url.as_deref()?;
            Some(IngressCandidateInput {
                node_id: node.id.to_string(),
                region: node.region.clone(),
                base_url: base_url.to_owned(),
                healthy: matches!(node.status, NodeStatus::Active)
                    && node.last_heartbeat_at.is_some_and(|heartbeat| {
                        (now - heartbeat).whole_seconds() <= HEARTBEAT_STALE_SECONDS
                    }),
                priority: 0,
            })
        })
        .collect::<Vec<_>>();
    Ok(healthy_candidates(&candidates, region))
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
mod tests {
    use super::{CnameGuidanceInput, IngressCandidateInput, cname_guidance, healthy_candidates};

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
