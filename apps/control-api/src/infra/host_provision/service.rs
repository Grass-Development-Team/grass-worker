//! Shared host binding orchestration used by project creation and the host
//! binding APIs: quota check, conflict check, binding row, provisioner call,
//! and provision event recording.

use grass_cache::CacheStore;
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use crate::{
    domain::{
        hosts::{self, CreateBindingParams, RecordProvisionEventParams},
        quotas::QuotaDimension,
    },
    infra::{
        database::entity::{
            HostBindingEnvironment, HostBindingKind, HostBindingStatus, HostProvisionEventStatus,
            host_source, project, project_host_binding, team,
        },
        error::AppError,
        quota::{QuotaCharge, QuotaService},
    },
};

use super::{
    CompositeHostProvisioner, HostProvisionError, HostProvisioner, ProvisionProjectHostInput,
};

pub struct BindHostRequest<'a> {
    pub project: &'a project::Model,
    pub team: &'a team::Model,
    pub source: Option<&'a host_source::Model>,
    pub host: String,
    pub kind: HostBindingKind,
    pub environment: HostBindingEnvironment,
    pub is_primary: bool,
    pub actor_user_id: Option<Uuid>,
}

pub struct HostBindingService<'a> {
    db: &'a DatabaseConnection,
    cache: &'a CacheStore,
    provisioner: CompositeHostProvisioner,
}

impl<'a> HostBindingService<'a> {
    pub fn new(db: &'a DatabaseConnection, cache: &'a CacheStore) -> Self {
        Self {
            db,
            cache,
            provisioner: CompositeHostProvisioner::new(),
        }
    }

    /// Creates a host binding: consumes host quota, rejects conflicting
    /// hosts, stores the binding, runs the provisioner, and records the
    /// provision event. Provider failures leave the binding in `failed`
    /// with a retry entry point instead of erroring the request.
    pub async fn bind_host(
        &self,
        op: &'static str,
        request: BindHostRequest<'_>,
    ) -> Result<project_host_binding::Model, AppError> {
        if hosts::find_binding_by_host(self.db, &request.host)
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?
            .is_some()
        {
            return Err(AppError::Conflict {
                op,
                message: format!("host {} is already bound", request.host),
            });
        }

        let quota = QuotaService::new(self.db, self.cache);
        let reservation = quota
            .reserve(
                op,
                request.team,
                request.actor_user_id,
                &[QuotaCharge::one(QuotaDimension::Hosts)],
            )
            .await?;

        let binding = match hosts::create_binding(
            self.db,
            CreateBindingParams {
                project_id: request.project.id,
                team_id: request.team.id,
                host_source_id: request.source.map(|source| source.id),
                host: request.host.clone(),
                kind: request.kind,
                environment: request.environment,
                status: HostBindingStatus::Pending,
                failure_reason: None,
                is_primary: request.is_primary,
            },
        )
        .await
        {
            Ok(binding) => binding,
            Err(source) => {
                quota.rollback(reservation).await;
                return Err(if crate::infra::database::is_unique_violation(&source) {
                    AppError::Conflict {
                        op,
                        message: format!("host {} is already bound", request.host),
                    }
                } else {
                    AppError::Infrastructure { op, source }
                });
            }
        };

        quota
            .commit(op, reservation, "project_host_binding", Some(binding.id))
            .await?;

        let binding = match request.source {
            Some(source) => self.provision(op, binding, source).await?,
            // Bindings without a source (custom hosts) activate directly;
            // DNS is the user's responsibility.
            None => hosts::update_binding_status(self.db, binding, HostBindingStatus::Active, None)
                .await
                .map_err(|source| AppError::Infrastructure { op, source })?,
        };

        Ok(binding)
    }

    /// Releases the provider side of a binding. Current provisioners are
    /// no-ops here, but the call keeps deprovisioning observable per source.
    pub async fn deprovision(
        &self,
        op: &'static str,
        binding: &project_host_binding::Model,
        source: &host_source::Model,
    ) -> Result<(), AppError> {
        if let Err(error) = self
            .provisioner
            .deprovision_project_host(super::DeprovisionProjectHostInput {
                host: &binding.host,
                source,
            })
            .await
        {
            tracing::warn!(operation = op, %error, host = %binding.host, "deprovision failed");
            hosts::record_provision_event(
                self.db,
                RecordProvisionEventParams {
                    host_binding_id: binding.id,
                    host_source_id: Some(source.id),
                    status: HostProvisionEventStatus::Failed,
                    operation: "host.deprovision".to_owned(),
                    provider_request_id: None,
                    error_code: Some("deprovision_failed".to_owned()),
                    error_message: Some(error.to_string()),
                    metadata: serde_json::json!({ "host": binding.host }),
                },
            )
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?;
        }
        Ok(())
    }

    /// Runs the provisioner for an existing binding and records the outcome.
    /// Used both at bind time and by the retry entry point.
    pub async fn provision(
        &self,
        op: &'static str,
        binding: project_host_binding::Model,
        source: &host_source::Model,
    ) -> Result<project_host_binding::Model, AppError> {
        let result = self
            .provisioner
            .provision_project_host(ProvisionProjectHostInput {
                host: &binding.host,
                source,
            })
            .await;

        let (status, event_status, request_id, message) = match &result {
            Ok(provisioned) => (
                provisioned.status.clone(),
                match provisioned.status {
                    HostBindingStatus::Active => HostProvisionEventStatus::Success,
                    _ => HostProvisionEventStatus::Pending,
                },
                provisioned.provider_request_id.clone(),
                provisioned.message.clone(),
            ),
            Err(HostProvisionError::Provider(message)) => (
                HostBindingStatus::Failed,
                HostProvisionEventStatus::Failed,
                None,
                Some(message.clone()),
            ),
            Err(HostProvisionError::UnsupportedSource(message)) => (
                HostBindingStatus::Failed,
                HostProvisionEventStatus::Failed,
                None,
                Some(message.clone()),
            ),
        };

        hosts::record_provision_event(
            self.db,
            RecordProvisionEventParams {
                host_binding_id: binding.id,
                host_source_id: Some(source.id),
                status: event_status.clone(),
                operation: "host.provision".to_owned(),
                provider_request_id: request_id,
                error_code: matches!(event_status, HostProvisionEventStatus::Failed)
                    .then(|| "provision_failed".to_owned()),
                error_message: matches!(event_status, HostProvisionEventStatus::Failed)
                    .then(|| message.clone().unwrap_or_default()),
                metadata: serde_json::json!({ "host": binding.host }),
            },
        )
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;

        let failure_reason = match status {
            HostBindingStatus::Failed | HostBindingStatus::Pending => message,
            _ => None,
        };
        hosts::update_binding_status(self.db, binding, status, failure_reason)
            .await
            .map_err(|source| AppError::Infrastructure { op, source })
    }
}
