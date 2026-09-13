use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::managed_certificate as cert;
use crate::infra::database::entity::{
    HostBindingKind, HostBindingStatus, HostReviewStatus, project_host_binding, regional_ingress,
};

pub(crate) fn ingress_fixture() -> regional_ingress::Model {
    regional_ingress::Model {
        id: Uuid::now_v7(),
        region: "eu".to_owned(),
        hostname: "entry.example.org".to_owned(),
        enabled: true,
        health_check_path: "/_grass/health".to_owned(),
        health_check_interval_seconds: 30,
        origin_host_preservation: true,
        dns_status: "resolved".to_owned(),
        dns_checked_at: Some(OffsetDateTime::now_utc()),
        dns_error: None,
        deleted_at: None,
        created_at: OffsetDateTime::now_utc(),
        updated_at: OffsetDateTime::now_utc(),
    }
}

pub(crate) fn certificate_fixture(ingress: &regional_ingress::Model) -> cert::Model {
    cert::Model {
        id: ingress.id,
        ingress_id: ingress.id,
        host_binding_id: None,
        hostname: ingress.hostname.clone(),
        issuer: "letsencrypt".to_owned(),
        contact_email: "owner@example.org".to_owned(),
        challenge_method: "http01".to_owned(),
        auto_renew: true,
        status: "pending".to_owned(),
        error: None,
        bundle: None,
        acme_account: None,
        revision: String::new(),
        issued_at: None,
        expires_at: None,
        retry_at: None,
        failure_count: 0,
        lease_until: None,
        generation: Uuid::now_v7(),
        challenge_token: None,
        challenge_value: None,
        challenge_expires_at: None,
        updated_at: OffsetDateTime::now_utc(),
    }
}

pub(crate) fn binding_fixture() -> project_host_binding::Model {
    project_host_binding::Model {
        id: Uuid::now_v7(),
        project_id: Uuid::now_v7(),
        team_id: Uuid::now_v7(),
        host_source_id: None,
        host: "site.example.org".to_owned(),
        region: "eu".to_owned(),
        kind: HostBindingKind::Custom,
        environment: crate::infra::database::entity::HostBindingEnvironment::Production,
        status: HostBindingStatus::Active,
        failure_reason: None,
        is_primary: false,
        review_status: HostReviewStatus::Approved,
        reviewed_by_user_id: None,
        reviewed_at: None,
        review_reason: None,
        ownership_status: "verified".to_owned(),
        ownership_checked_at: None,
        ownership_error: None,
        deleted_at: None,
        created_at: OffsetDateTime::now_utc(),
        updated_at: OffsetDateTime::now_utc(),
    }
}
