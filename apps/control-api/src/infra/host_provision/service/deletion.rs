use sea_orm::TransactionTrait;
use serde_json::json;
use uuid::Uuid;

use super::HostBindingService;
use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        hosts, notifications, projects,
        quotas::QuotaDimension,
    },
    infra::{
        database::entity::AuditEventResult,
        error::AppError,
        quota::{QuotaCharge, QuotaService},
        route_invalidation,
    },
};

/// The caller authorizes the operation; this scope also constrains project lookups.
pub enum DeleteHostScope {
    Project(Uuid),
    Platform {
        actor_user_id: Uuid,
        reason: Option<String>,
    },
}

impl HostBindingService<'_> {
    /// Commits a tombstone and its administrator audit before external cleanup.
    /// Repeating DELETE retries cleanup without duplicating the audit or quota release.
    pub async fn delete_host(
        &self,
        op: &'static str,
        binding_id: Uuid,
        scope: DeleteHostScope,
    ) -> Result<(), AppError> {
        let transaction = self
            .db
            .begin()
            .await
            .map_err(|source| AppError::Infrastructure {
                op,
                source: source.into(),
            })?;
        let mut binding =
            hosts::get_binding_by_id_for_update_including_deleted(&transaction, binding_id)
                .await
                .map_err(|source| AppError::Infrastructure { op, source })?
                .filter(|binding| match &scope {
                    DeleteHostScope::Project(project_id) => binding.project_id == *project_id,
                    DeleteHostScope::Platform { .. } => true,
                })
                .ok_or_else(|| AppError::NotFound {
                    op,
                    message: match &scope {
                        DeleteHostScope::Project(_) => "host binding not found",
                        DeleteHostScope::Platform { .. } => "domain binding not found",
                    }
                    .to_owned(),
                })?;
        if binding.deleted_at.is_none() {
            binding = hosts::soft_delete_binding_at(
                &transaction,
                binding,
                time::OffsetDateTime::now_utc(),
            )
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?;
            if let DeleteHostScope::Platform {
                actor_user_id,
                reason,
            } = scope
            {
                audits::create_platform_audit_event_with_changes(
                    &transaction,
                    CreateAuditEventParams {
                        actor_user_id: Some(actor_user_id), actor_node_id: None, team_id: Some(binding.team_id),
                        action: "domain.deleted".to_owned(), target_type: "project_host_binding".to_owned(), target_id: Some(binding.id),
                        result: AuditEventResult::Success, reason: reason.clone(), metadata: json!({ "platform_admin": true, "project_id": binding.project_id }),
                    },
                    json!({ "before": { "host": binding.host, "deleted": false }, "after": { "deleted": true } }),
                ).await.map_err(|source| AppError::Infrastructure { op, source })?;
                let project = projects::get_by_id_any(&transaction, binding.project_id)
                    .await
                    .map_err(|source| AppError::Infrastructure { op, source })?
                    .ok_or_else(|| AppError::NotFound {
                        op,
                        message: "project not found".to_owned(),
                    })?;
                notifications::create_project_notification(
                    &transaction,
                    notifications::CreateProjectNotification {
                        project: &project,
                        actor_user_id,
                        action: "domain.deleted",
                        reason,
                        target_url: format!("/projects/{}/domains", project.id),
                    },
                )
                .await
                .map_err(|source| AppError::Infrastructure { op, source })?;
            }
        }
        // Use the value returned by the database: PostgreSQL timestamps have
        // microsecond precision, so a newly generated nanosecond value is not
        // necessarily the same value a later retry will load.
        let generation = binding.deleted_at.ok_or_else(|| AppError::Internal {
            op,
            message: "deleted binding has no deletion timestamp".to_owned(),
        })?;
        transaction
            .commit()
            .await
            .map_err(|source| AppError::Infrastructure {
                op,
                source: source.into(),
            })?;

        let dns_result = async {
            if let Some(source_id) = binding.host_source_id
                && let Some(source) = hosts::get_source_by_id_including_deleted(self.db, source_id)
                    .await
                    .map_err(|source| AppError::Infrastructure { op, source })?
            {
                self.deprovision(op, &binding, &source).await?;
            }
            Ok::<(), AppError>(())
        }
        .await;
        let quota_result = QuotaService::new(self.db, self.cache)
            .release_once_for_generation(
                op,
                binding.team_id,
                &[QuotaCharge::one(QuotaDimension::Hosts)],
                "project_host_binding",
                binding.id,
                generation,
            )
            .await;
        // Cleanup failures must not prevent withdrawal of a committed tombstone.
        // Infrastructure failures are returned so the same DELETE can retry them;
        // provider failures are recorded by deprovision for later cleanup.
        let route_result = route_invalidation::invalidate_project(
            self.db,
            self.platform_secret,
            binding.project_id,
        )
        .await
        .map_err(|source| AppError::Infrastructure { op, source });
        dns_result?;
        quota_result?;
        route_result
    }
}

#[cfg(test)]
mod tests;
