use sea_orm::ConnectionTrait;
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{notifications, projects},
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, project},
        error::AppError,
    },
    state::ControlApiState,
};

pub(crate) async fn archive(
    state: &ControlApiState,
    actor_user_id: Uuid,
    project_id: Uuid,
) -> Result<project::Model, AppError> {
    const OP: &str = "admin.projects.archive";
    let db = crate::infra::http::database(state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let project = load_project(&transaction, project_id, OP).await?;
    if project.archived_at.is_some() {
        return Err(AppError::Conflict {
            op: OP,
            message: "project is already archived".to_owned(),
        });
    }
    let project = projects::set_archived(&transaction, project, true)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_project_event(
        &transaction,
        actor_user_id,
        &project,
        "project.archived",
        format!("/projects/{}", project.id),
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(project)
}

pub(crate) async fn unarchive(
    state: &ControlApiState,
    actor_user_id: Uuid,
    project_id: Uuid,
) -> Result<project::Model, AppError> {
    const OP: &str = "admin.projects.unarchive";
    let db = crate::infra::http::database(state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let project = load_project(&transaction, project_id, OP).await?;
    if project.archived_at.is_none() {
        return Err(AppError::Conflict {
            op: OP,
            message: "project is not archived".to_owned(),
        });
    }
    let project = projects::set_archived(&transaction, project, false)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_project_event(
        &transaction,
        actor_user_id,
        &project,
        "project.unarchived",
        format!("/projects/{}", project.id),
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(project)
}

pub(crate) async fn remove(
    state: &ControlApiState,
    actor_user_id: Uuid,
    project_id: Uuid,
) -> Result<Vec<crate::domain::project_lifecycle::ProjectCleanupWarning>, AppError> {
    const OP: &str = "admin.projects.delete";
    let db = crate::infra::http::database(state, OP)?;
    let cache = crate::infra::http::cache(state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let project = load_project_any(&transaction, project_id, OP).await?;
    let deletion =
        crate::domain::project_lifecycle::soft_delete_project_records(&transaction, project)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    if deletion.newly_deleted {
        record_project_event(
            &transaction,
            actor_user_id,
            &deletion.project,
            "project.deleted",
            "/projects".to_owned(),
        )
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    }
    let project = deletion.project;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let warnings = if deletion.newly_deleted {
        let platform_secret = state.config.read().unwrap().secrets.secret_key.clone();
        crate::domain::project_lifecycle::finalize_deleted_project_resources(
            db,
            cache,
            &platform_secret,
            OP,
            &project,
            &deletion.bindings,
        )
        .await?
    } else {
        crate::domain::project_lifecycle::release_deleted_project_quota(
            db,
            cache,
            OP,
            &project,
            &deletion.bindings,
        )
        .await?;
        Vec::new()
    };
    Ok(warnings)
}

pub(crate) async fn load_project<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    op: &'static str,
) -> Result<project::Model, AppError> {
    projects::get_by_id(db, project_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "project not found".to_owned(),
        })
}

pub(crate) async fn load_project_any<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    op: &'static str,
) -> Result<project::Model, AppError> {
    projects::get_by_id_any(db, project_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "project not found".to_owned(),
        })
}

pub(crate) async fn record_project_event<C: crate::infra::audit::AuditConnection>(
    db: &C,
    actor: Uuid,
    project: &project::Model,
    action: &str,
    target_url: String,
) -> anyhow::Result<()> {
    audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(actor),
            actor_node_id: None,
            team_id: Some(project.team_id),
            action: action.to_owned(),
            target_type: "project".to_owned(),
            target_id: Some(project.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "platform_admin": true, "slug": project.slug }),
        },
    )
    .await?;
    notifications::create_project_notification(
        db,
        notifications::CreateProjectNotification {
            project,
            actor_user_id: actor,
            action,
            reason: None,
            target_url,
        },
    )
    .await?;
    Ok(())
}
