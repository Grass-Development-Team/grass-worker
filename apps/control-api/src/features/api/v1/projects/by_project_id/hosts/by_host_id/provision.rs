use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::EntityTrait;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::{certificates, host_bindings::HostBindingService, hosts, ingress},
    infra::{
        database::entity::{
            HostBindingEnvironment, HostBindingKind, HostBindingStatus, HostReviewStatus,
            project_host_binding,
        },
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/hosts/{host_id}/provision",
        axum::routing::post(provision),
    )
}

fn binding_view(binding: &project_host_binding::Model) -> HostBindingResponse {
    HostBindingResponse {
        id: binding.id,
        project_id: binding.project_id,
        host: binding.host.clone(),
        region: binding.region.clone(),
        kind: match binding.kind {
            HostBindingKind::Platform => "platform",
            HostBindingKind::Custom => "custom",
        },
        environment: match binding.environment {
            HostBindingEnvironment::Production => "production",
            HostBindingEnvironment::Preview => "preview",
            HostBindingEnvironment::All => "all",
        },
        status: status_value(&binding.status),
        failure_reason: binding.failure_reason.clone(),
        is_primary: binding.is_primary,
        host_source_id: binding.host_source_id,
        review_status: review_status_value(&binding.review_status),
        reviewed_by_user_id: binding.reviewed_by_user_id,
        reviewed_at: binding.reviewed_at,
        review_reason: binding.review_reason.clone(),
        ownership_status: binding.ownership_status.clone(),
        ownership_checked_at: binding.ownership_checked_at,
        ownership_error: binding.ownership_error.clone(),
        created_at: binding.created_at,
        ingress: None,
        onboarding: None,
        connection_state: None,
        certificate: None,
    }
}

async fn attach_ingress_guidance(
    state: &ControlApiState,
    db: &sea_orm::DatabaseConnection,
    binding: &project_host_binding::Model,
    mut view: HostBindingResponse,
    op: &'static str,
) -> Result<HostBindingResponse, AppError> {
    use crate::infra::database::entity::{
        managed_certificate, node_ingress_status, regional_ingress,
    };
    use sea_orm::{ColumnTrait, QueryFilter};
    if !matches!(binding.kind, HostBindingKind::Custom) {
        return Ok(view);
    }
    let connection = crate::domain::domain_onboarding::get(db, binding.id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    view.onboarding = Some(connection.as_ref().map(|record| OnboardingResponse {
        dns_status: record.dns_status.clone(),
        dns_error: record.dns_error.clone(),
        checked_at: record.checked_at,
        next_check_at: record.next_check_at,
    }));
    view.connection_state = Some("entry_unavailable".to_owned());
    let Some(entry) = regional_ingress::Entity::find()
        .filter(regional_ingress::Column::Region.eq(&binding.region))
        .filter(regional_ingress::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?
    else {
        return Ok(view);
    };
    let now = OffsetDateTime::now_utc();
    let candidates = ingress::healthy_serve_nodes(db, &binding.region, now)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    let secret = state.config.read().unwrap().secrets.secret_key.clone();
    let issuer = crate::domain::certificate_settings::issuer(db)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    let record = managed_certificate::Entity::find_by_id(binding.id)
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    let mut installed = 0;
    if let Some(record) = &record {
        for candidate in &candidates {
            let node_id =
                Uuid::parse_str(&candidate.node_id).map_err(|source| AppError::Infrastructure {
                    op,
                    source: source.into(),
                })?;
            let status = node_ingress_status::Entity::find_by_id(node_id)
                .one(db)
                .await
                .map_err(|source| AppError::Infrastructure {
                    op,
                    source: source.into(),
                })?;
            if status.as_ref().is_some_and(|s| {
                s.tls_ready
                    && now - s.checked_at < time::Duration::seconds(30)
                    && s.certificates.as_array().is_some_and(|certs| {
                        certs.iter().any(|c| {
                            c["ingress_id"] == json!(record.id) && c["revision"] == record.revision
                        })
                    })
            }) {
                installed += 1;
            }
        }
    }
    let https_ready = entry.enabled
        && certificates::binding_eligible(binding)
        && !candidates.is_empty()
        && installed == candidates.len()
        && record.as_ref().is_some_and(|c| {
            !c.revision.is_empty() && c.expires_at.is_some_and(|expiry| expiry > now)
        });
    let mut certificate = CertificateResponse::from_record(record.as_ref(), &entry, &issuer);
    certificate.https_ready = Some(https_ready);
    certificate.installed_nodes = Some(installed);
    certificate.required_nodes = Some(candidates.len());
    view.certificate = Some(certificate.clone());
    let guidance = ingress::cname_guidance(ingress::CnameGuidanceInput {
        host: &binding.host,
        region: &binding.region,
        ingress_hostname: &entry.hostname,
        verification_name: &format!("_grass.{}", binding.host),
        verification_value: &ingress::dns_verification_token(&secret, binding.id, &binding.host),
        origin_host_preservation: true,
    });
    view.ingress = Some(IngressResponse {
        region: guidance.region,
        enabled: entry.enabled,
        cname: CnameResponse {
            record_type: guidance.record_type,
            name: guidance.name,
            target: guidance.target,
        },
        txt: TxtResponse {
            record_type: "TXT",
            name: guidance.verification_name,
            value: guidance.verification_value,
        },
        origin_host_preservation: guidance.origin_host_preservation,
        health_check: HealthCheckResponse {
            path: entry.health_check_path,
            interval_seconds: entry.health_check_interval_seconds,
        },
        entrance_nodes: candidates
            .iter()
            .map(|node| EntranceNodeResponse {
                node_id: node.node_id.clone(),
                base_url: node.base_url.clone(),
                priority: node.priority,
            })
            .collect(),
        certificate,
    });
    view.connection_state = Some(
        (if binding.status == HostBindingStatus::Disabled {
            "disabled"
        } else if !entry.enabled {
            "entry_unavailable"
        } else if connection.is_none() {
            "contact_missing"
        } else if let Some(connection) = connection.as_ref().filter(|c| c.dns_status != "ready") {
            connection.dns_status.as_str()
        } else if binding.ownership_status != "verified" {
            "ownership_pending"
        } else if !matches!(
            binding.review_status,
            HostReviewStatus::Approved | HostReviewStatus::NotRequired
        ) {
            "review_pending"
        } else if candidates.is_empty() {
            "entry_unavailable"
        } else if https_ready {
            "ready"
        } else {
            match record.as_ref().map(|c| c.status.as_str()) {
                Some("issuing") => "issuing",
                Some("active") => "installing",
                Some("failed") => "certificate_failed",
                _ => "certificate_pending",
            }
        })
        .to_owned(),
    );
    Ok(view)
}

fn review_status_value(status: &HostReviewStatus) -> &'static str {
    match status {
        HostReviewStatus::NotRequired => "not_required",
        HostReviewStatus::Pending => "pending",
        HostReviewStatus::Approved => "approved",
        HostReviewStatus::Rejected => "rejected",
    }
}

fn status_value(status: &HostBindingStatus) -> &'static str {
    match status {
        HostBindingStatus::Pending => "pending",
        HostBindingStatus::Active => "active",
        HostBindingStatus::Failed => "failed",
        HostBindingStatus::Disabled => "disabled",
    }
}

/// POST /api/v1/projects/{project_id}/hosts/{host_id}/provision — retry
/// provisioning for pending or failed bindings.
pub async fn provision(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, host_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.provision";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    access.require_member(OP)?;
    let db = crate::infra::http::database(&state, OP)?;
    let cache = crate::infra::http::cache(&state, OP)?;

    let binding = load_binding(db, &access, host_id, OP).await?;
    let Some(source_id) = binding.host_source_id else {
        return Err(AppError::Validation {
            op: OP,
            message: "custom hosts have no provisioner to run".to_owned(),
        });
    };
    let source = hosts::get_source_by_id(db, source_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "host source not found".to_owned(),
        })?;

    let platform_secret = state.config.read().unwrap().secrets.secret_key.clone();
    let service = HostBindingService::new(db, cache, &platform_secret);
    let binding = service.provision(OP, binding, &source).await?;

    let view = attach_ingress_guidance(&state, db, &binding, binding_view(&binding), OP).await?;
    Ok(ok_response(ProvisionResponse { host: view }))
}

pub(super) async fn load_binding(
    db: &sea_orm::DatabaseConnection,
    access: &crate::domain::project_access::ProjectAccess,
    host_id: Uuid,
    op: &'static str,
) -> Result<project_host_binding::Model, AppError> {
    let binding = hosts::get_binding_by_id(db, host_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "host binding not found".to_owned(),
        })?;
    if binding.project_id != access.project.id {
        return Err(AppError::NotFound {
            op,
            message: "host binding not found".to_owned(),
        });
    }
    Ok(binding)
}

#[derive(serde::Serialize)]
struct HostBindingResponse {
    id: Uuid,
    project_id: Uuid,
    host: String,
    region: String,
    kind: &'static str,
    environment: &'static str,
    status: &'static str,
    failure_reason: Option<String>,
    is_primary: bool,
    host_source_id: Option<Uuid>,
    review_status: &'static str,
    reviewed_by_user_id: Option<Uuid>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    reviewed_at: Option<time::OffsetDateTime>,
    review_reason: Option<String>,
    ownership_status: String,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    ownership_checked_at: Option<time::OffsetDateTime>,
    ownership_error: Option<String>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
    ingress: Option<IngressResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    onboarding: Option<Option<OnboardingResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    connection_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    certificate: Option<CertificateResponse>,
}

#[derive(serde::Serialize)]
struct OnboardingResponse {
    dns_status: String,
    dns_error: Option<String>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    checked_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    next_check_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct IngressResponse {
    region: String,
    enabled: bool,
    cname: CnameResponse,
    txt: TxtResponse,
    origin_host_preservation: bool,
    health_check: HealthCheckResponse,
    entrance_nodes: Vec<EntranceNodeResponse>,
    certificate: CertificateResponse,
}

#[derive(serde::Serialize)]
struct CnameResponse {
    record_type: &'static str,
    name: String,
    target: String,
}
#[derive(serde::Serialize)]
struct TxtResponse {
    record_type: &'static str,
    name: String,
    value: String,
}
#[derive(serde::Serialize)]
struct HealthCheckResponse {
    path: String,
    interval_seconds: i32,
}
#[derive(serde::Serialize)]
struct EntranceNodeResponse {
    node_id: String,
    base_url: String,
    priority: i32,
}
#[derive(Clone, serde::Serialize)]
struct CertificateResponse {
    enabled: bool,
    status: String,
    issuer: String,
    platform_issuer: String,
    challenge_method: &'static str,
    auto_renew: bool,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    issued_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    expires_at: Option<time::OffsetDateTime>,
    error: Option<String>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    retry_at: Option<time::OffsetDateTime>,
    revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    https_ready: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    installed_nodes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    required_nodes: Option<usize>,
}

impl CertificateResponse {
    fn from_record(
        item: Option<&crate::infra::database::entity::managed_certificate::Model>,
        ingress: &crate::infra::database::entity::regional_ingress::Model,
        issuer: &str,
    ) -> Self {
        Self {
            enabled: ingress.enabled,
            status: if !ingress.enabled || ingress.deleted_at.is_some() {
                "disabled".to_owned()
            } else {
                item.map(|item| item.status.clone())
                    .unwrap_or_else(|| "pending".to_owned())
            },
            issuer: item
                .map(|item| item.issuer.clone())
                .unwrap_or_else(|| issuer.to_owned()),
            platform_issuer: issuer.to_owned(),
            challenge_method: "http01",
            auto_renew: item.is_none_or(|item| item.auto_renew),
            issued_at: item.and_then(|item| item.issued_at),
            expires_at: item.and_then(|item| item.expires_at),
            error: item.and_then(|item| item.error.clone()),
            retry_at: item.and_then(|item| item.retry_at),
            revision: item.map(|item| item.revision.clone()).unwrap_or_default(),
            https_ready: None,
            installed_nodes: None,
            required_nodes: None,
        }
    }
}

#[derive(serde::Serialize)]
struct ProvisionResponse {
    host: HostBindingResponse,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::database::entity::HostBindingEnvironment;
    use crate::infra::database::entity::HostBindingKind;
    use crate::infra::database::entity::HostBindingStatus;
    use crate::infra::database::entity::HostReviewStatus;
    use crate::infra::database::entity::project_host_binding;
    use time::OffsetDateTime;
    use uuid::Uuid;
    #[test]
    fn host_view_exposes_review_state_separately_from_provisioning() {
        let reviewer_id = Uuid::now_v7();
        let binding = project_host_binding::Model {
            id: Uuid::now_v7(),
            project_id: Uuid::now_v7(),
            team_id: Uuid::now_v7(),
            host_source_id: None,
            host: "manual.example.test".to_owned(),
            region: "default".to_owned(),
            kind: HostBindingKind::Custom,
            environment: HostBindingEnvironment::Production,
            status: HostBindingStatus::Pending,
            failure_reason: None,
            is_primary: false,
            review_status: HostReviewStatus::Rejected,
            reviewed_by_user_id: Some(reviewer_id),
            reviewed_at: Some(OffsetDateTime::UNIX_EPOCH),
            review_reason: Some("Ownership could not be verified".to_owned()),
            ownership_status: "pending".to_owned(),
            ownership_checked_at: None,
            ownership_error: None,
            deleted_at: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        };

        let view = serde_json::to_value(binding_view(&binding)).unwrap();

        assert_eq!(view["status"], "pending");
        assert_eq!(view["review_status"], "rejected");
        assert_eq!(view["reviewed_by_user_id"], reviewer_id.to_string());
        assert!(view["reviewed_at"].is_string());
        assert_eq!(view["review_reason"], "Ownership could not be verified");
    }

    #[tokio::test]
    async fn custom_host_without_ingress_preserves_null_and_absent_fields() {
        use crate::infra::database::entity::{domain_onboarding, regional_ingress};
        use sea_orm::{DbBackend, MockDatabase};

        let binding = crate::domain::certificates::tests::binding_fixture();
        let db = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([Vec::<domain_onboarding::Model>::new()])
            .append_query_results([Vec::<regional_ingress::Model>::new()])
            .into_connection();
        let state = ControlApiState::new(Default::default(), "unused-host-contract.toml");
        let view = attach_ingress_guidance(
            &state,
            &db,
            &binding,
            binding_view(&binding),
            "test.host.contract",
        )
        .await
        .unwrap();
        let value = serde_json::to_value(view).unwrap();
        assert_eq!(value["connection_state"], "entry_unavailable");
        assert!(value.as_object().unwrap().contains_key("onboarding"));
        assert!(value["onboarding"].is_null());
        assert!(value["ingress"].is_null());
        assert!(!value.as_object().unwrap().contains_key("certificate"));
    }

    #[tokio::test]
    async fn domain_onboarding_response_omits_the_certificate_contact() {
        use crate::infra::database::entity::{domain_onboarding, regional_ingress};
        use sea_orm::{DbBackend, MockDatabase};
        let binding = crate::domain::certificates::tests::binding_fixture();
        let db = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([vec![domain_onboarding::Model {
                binding_id: binding.id,
                created_by_user_id: None,
                contact_email: "private-contact@example.test".to_owned(),
                dns_status: "unresolved".to_owned(),
                dns_error: None,
                checked_at: None,
                next_check_at: time::OffsetDateTime::UNIX_EPOCH,
                lease_until: None,
            }]])
            .append_query_results([Vec::<regional_ingress::Model>::new()])
            .into_connection();
        let state = ControlApiState::new(Default::default(), "unused-host-contract.toml");
        let response = attach_ingress_guidance(
            &state,
            &db,
            &binding,
            binding_view(&binding),
            "test.host.contract",
        )
        .await
        .unwrap();
        let value = serde_json::to_value(response).unwrap();
        assert_eq!(value["onboarding"]["dns_status"], "unresolved");
        assert!(!value.to_string().contains("private-contact"));
        assert!(
            !value["onboarding"]
                .as_object()
                .unwrap()
                .contains_key("contact_email")
        );
    }
}
