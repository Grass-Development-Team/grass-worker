pub(crate) mod by_host_id;

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::EntityTrait;
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::{
        certificates, deployments,
        host_bindings::{BindHostRequest, HostBindingService},
        hosts::{self, DomainReviewMode},
        ingress,
    },
    infra::{
        database::entity::{
            DeploymentEnvironment, HostBindingEnvironment, HostBindingKind, HostBindingStatus,
            HostReviewStatus, host_policy, project_host_binding,
        },
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route(
            "/projects/{project_id}/hosts",
            axum::routing::get(list).post(create),
        )
        .merge(by_host_id::router())
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
        serving: None,
        provision_events: None,
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

fn parse_environment(value: &str, op: &'static str) -> Result<HostBindingEnvironment, AppError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "production" => Ok(HostBindingEnvironment::Production),
        "preview" => Ok(HostBindingEnvironment::Preview),
        "all" => Ok(HostBindingEnvironment::All),
        other => Err(AppError::Validation {
            op,
            message: format!("invalid environment: {other}"),
        }),
    }
}

/// GET /api/v1/projects/{project_id}/hosts
async fn list(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.list";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    let db = crate::infra::http::database(&state, OP)?;

    let bindings = hosts::list_bindings_for_project(db, access.project.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let production_deployment =
        deployments::find_active(db, access.project.id, DeploymentEnvironment::Production)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let preview_deployment =
        deployments::find_active(db, access.project.id, DeploymentEnvironment::Preview)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    let mut views = Vec::with_capacity(bindings.len());
    for binding in &bindings {
        let events = hosts::list_provision_events_for_binding(db, binding.id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        let view = binding_view(binding);
        let mut view = attach_ingress_guidance(&state, db, binding, view, OP).await?;
        view.serving = Some(
            matches!(binding.status, HostBindingStatus::Active)
                && match binding.environment {
                    HostBindingEnvironment::Production => production_deployment.is_some(),
                    HostBindingEnvironment::Preview => preview_deployment.is_some(),
                    HostBindingEnvironment::All => {
                        production_deployment.is_some() || preview_deployment.is_some()
                    }
                },
        );
        view.provision_events = Some(
            events
                .iter()
                .take(10)
                .map(|event| ProvisionEventResponse {
                    id: event.id,
                    status: match event.status {
                        crate::infra::database::entity::HostProvisionEventStatus::Success => {
                            "success"
                        }
                        crate::infra::database::entity::HostProvisionEventStatus::Pending => {
                            "pending"
                        }
                        crate::infra::database::entity::HostProvisionEventStatus::Failed => {
                            "failed"
                        }
                    },
                    operation: event.operation.clone(),
                    error_message: event.error_message.clone(),
                    created_at: event.created_at,
                })
                .collect::<Vec<_>>(),
        );
        views.push(view);
    }

    Ok(ok_response(ListResponse { hosts: views }))
}

#[derive(Deserialize)]
struct CreateHostRequest {
    host: String,
    #[serde(default)]
    region: Option<String>,
    #[serde(default = "default_environment")]
    environment: String,
    #[serde(default)]
    host_source_id: Option<Uuid>,
}

fn default_environment() -> String {
    "production".to_owned()
}

/// POST /api/v1/projects/{project_id}/hosts
async fn create(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
    Json(body): Json<CreateHostRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.create";
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

    let host =
        grass_validator::normalize_host(&body.host).map_err(|error| AppError::Validation {
            op: OP,
            message: error.to_string(),
        })?;
    let environment = parse_environment(&body.environment, OP)?;
    let source = match body.host_source_id {
        Some(source_id) => Some(
            hosts::get_source_by_id(db, source_id)
                .await
                .map_err(|source| AppError::Infrastructure { op: OP, source })?
                .ok_or_else(|| AppError::NotFound {
                    op: OP,
                    message: "host source not found".to_owned(),
                })?,
        ),
        None => None,
    };
    let region = body
        .region
        .as_deref()
        .map(grass_validator::normalize_region)
        .transpose()
        .map_err(|error| AppError::Validation {
            op: OP,
            message: format!("region: {error}"),
        })?
        .or_else(|| source.as_ref().map(|source| source.region.clone()))
        .unwrap_or_else(|| "default".to_owned());

    if let Some(source) = source.as_ref()
        && region != source.region
    {
        return Err(AppError::Validation {
            op: OP,
            message: format!(
                "region must match the host source region ({})",
                source.region
            ),
        });
    }

    crate::domain::regions::require(db, &region, OP).await?;
    if source.is_none()
        && ingress::get_enabled_by_region(db, &region)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?
            .is_none()
    {
        return Err(AppError::Validation { op: OP, message: "This region has no enabled CNAME entry. Select an available region or contact a platform administrator.".to_owned() });
    }
    // Custom hosts require the team group policy to allow them; hosts under
    // a platform source must live under that source's base domain.
    let policy = hosts::policy_for_team_group(db, access.team.group_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    match &source {
        Some(source) => {
            if !host.ends_with(&format!(".{}", source.base_domain)) {
                return Err(AppError::Validation {
                    op: OP,
                    message: format!(
                        "host must be a subdomain of {} for this host source",
                        source.base_domain
                    ),
                });
            }
        }
        None => {
            if let Some(host_policy::Model {
                allow_custom_hosts: false,
                ..
            }) = policy
            {
                return Err(AppError::Forbidden {
                    op: OP,
                    message: "team group does not allow custom hosts".to_owned(),
                });
            }
        }
    }

    let existing = hosts::list_bindings_for_project(db, access.project.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let is_primary = existing.is_empty();
    let review_status = if source.is_some() {
        HostReviewStatus::NotRequired
    } else {
        match hosts::domain_review_policy_for_team(db, access.team.id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?
        {
            DomainReviewMode::Auto => HostReviewStatus::Approved,
            DomainReviewMode::Manual => HostReviewStatus::Pending,
        }
    };

    let platform_secret = state.config.read().unwrap().secrets.secret_key.clone();
    let service = HostBindingService::new(db, cache, &platform_secret);
    let binding = service
        .bind_host(
            OP,
            BindHostRequest {
                project: &access.project,
                team: &access.team,
                source: source.as_ref(),
                host,
                region,
                kind: if source.is_some() {
                    HostBindingKind::Platform
                } else {
                    HostBindingKind::Custom
                },
                environment,
                is_primary,
                review_status,
                actor_user_id: Some(session.data.user_id),
            },
        )
        .await?;

    if binding.kind == HostBindingKind::Custom
        && crate::domain::domain_onboarding::run_check(db, binding.id, &platform_secret, true)
            .await
            .is_err()
    {
        tracing::warn!(operation="projects.hosts.initial_check", binding_id=%binding.id, "Initial domain check will retry in the background");
    }
    let binding = hosts::get_binding_by_id(db, binding.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "Domain binding was removed".to_owned(),
        })?;
    let view = attach_ingress_guidance(&state, db, &binding, binding_view(&binding), OP).await?;
    Ok(ok_response(CreateResponse { host: view }))
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
    #[serde(skip_serializing_if = "Option::is_none")]
    serving: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provision_events: Option<Vec<ProvisionEventResponse>>,
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
struct ProvisionEventResponse {
    id: Uuid,
    status: &'static str,
    operation: String,
    error_message: Option<String>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct ListResponse {
    hosts: Vec<HostBindingResponse>,
}

#[derive(serde::Serialize)]
struct CreateResponse {
    host: HostBindingResponse,
}
