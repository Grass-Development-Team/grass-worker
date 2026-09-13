use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{
        deployments::{self, CreateDeploymentParams},
        hosts,
        scheduler::{self, PlacementMode},
    },
    infra::{
        audit::{self as audits, CreateAuditEventParams},
        database::entity::{AuditEventResult, DeploymentEnvironment, deployment},
        error::AppError,
        http::deployment_errors::map_schedule_error,
    },
};

pub(crate) async fn create_placed_deployment(
    db: &sea_orm::DatabaseConnection,
    params: CreateDeploymentParams,
    selected_node_id: Option<Uuid>,
    region: Option<&str>,
    op: &'static str,
) -> Result<deployment::Model, AppError> {
    let requested = deployments::runtime_serve_resources(&params.project.runtime);
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    let placement = match scheduler::place_deployment_in_region(
        &transaction,
        requested,
        selected_node_id,
        region,
    )
    .await
    {
        Ok(placement) => placement,
        Err(error) => {
            let error = map_schedule_error(error, op);
            let _ = transaction.rollback().await;
            return Err(error);
        }
    };
    let placement_for_event = placement.clone();
    let deployment = match deployments::create_deployment(&transaction, params, placement).await {
        Ok(deployment) => deployment,
        Err(source) => {
            let _ = transaction.rollback().await;
            return Err(AppError::Infrastructure { op, source });
        }
    };
    let mode = match placement_for_event.mode {
        PlacementMode::Automatic => "automatic",
        PlacementMode::Manual => "manual",
    };
    if let Err(source) = deployments::append_event(
        &transaction,
        deployment.id,
        crate::infra::database::entity::DeploymentEventKind::Serve,
        "deployment assigned to serve node",
        json!({
            "mode": mode,
            "serve_node_id": placement_for_event.node_id,
            "resources": requested,
            "overcommitted": placement_for_event.overcommitted,
        }),
    )
    .await
    {
        let _ = transaction.rollback().await;
        return Err(AppError::Infrastructure { op, source });
    }
    audits::create_audit_event(
        &transaction,
        CreateAuditEventParams {
            actor_user_id: deployment.triggered_by_user_id,
            actor_node_id: None,
            team_id: Some(deployment.team_id),
            action: "deployment.created".to_owned(),
            target_type: "deployment".to_owned(),
            target_id: Some(deployment.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({
                "project_id": deployment.project_id,
                "environment": deployments::environment_value(&deployment.environment),
            }),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    Ok(deployment)
}

pub(crate) async fn preview_host_for_project(
    db: &sea_orm::DatabaseConnection,
    project: &crate::infra::database::entity::project::Model,
) -> Option<String> {
    let sources = hosts::list_sources(db).await.ok()?;
    match hosts::select_auto_assign_source(&sources) {
        hosts::AutoAssignSelection::Source(source) => {
            // The host embeds the deployment id, which is generated inside
            // create_deployment; pre-generate one here and thread it through
            // instead would complicate creation, so derive from a fresh UUID
            // and store it directly on the deployment row at creation time.
            Some(hosts::preview_host_for(
                &project.slug,
                Uuid::now_v7(),
                &source.base_domain,
            ))
        }
        _ => None,
    }
}

pub(crate) fn environment_gets_preview_host(environment: &DeploymentEnvironment) -> bool {
    matches!(
        environment,
        DeploymentEnvironment::Production | DeploymentEnvironment::Preview
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_environment_gets_a_protected_preview_host() {
        assert!(environment_gets_preview_host(
            &DeploymentEnvironment::Preview
        ));
        assert!(environment_gets_preview_host(
            &DeploymentEnvironment::Production
        ));
    }

    #[test]
    fn static_and_ssr_requests_use_fixed_first_phase_resources() {
        assert_eq!(
            deployments::runtime_serve_resources(
                &crate::infra::database::entity::ProjectRuntime::Static
            ),
            grass_node_protocol::ServeResources {
                cpu_millicores: 50,
                memory_mb: 64,
                disk_mb: 256,
            }
        );
        assert_eq!(
            deployments::runtime_serve_resources(
                &crate::infra::database::entity::ProjectRuntime::Ssr
            ),
            grass_node_protocol::ServeResources {
                cpu_millicores: 200,
                memory_mb: 256,
                disk_mb: 512,
            }
        );
    }
}
