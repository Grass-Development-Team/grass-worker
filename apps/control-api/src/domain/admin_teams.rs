use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::teams,
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, TeamKind, project, team},
        error::AppError,
    },
    state::ControlApiState,
};

pub(crate) async fn remove(
    state: &ControlApiState,
    actor_user_id: Uuid,
    team_id: Uuid,
) -> Result<(), AppError> {
    const OP: &str = "admin.teams.remove";
    let db = crate::infra::http::database(state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;

    let team = teams::get_by_id(db, team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "team not found".to_owned(),
        })?;

    if team.kind == TeamKind::Personal {
        return Err(AppError::Validation {
            op: OP,
            message: "personal teams cannot be deleted".to_owned(),
        });
    }
    let project_count = project::Entity::find()
        .filter(project::Column::TeamId.eq(team.id))
        .filter(project::Column::DeletedAt.is_null())
        .count(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    if project_count > 0 {
        return Err(AppError::Conflict {
            op: OP,
            message: format!(
                "team still owns {project_count} project(s); delete or transfer them first"
            ),
        });
    }

    teams::soft_delete(db, team.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(actor_user_id),
            actor_node_id: None,
            team_id: Some(team.id),
            action: "team.deleted".to_owned(),
            target_type: "team".to_owned(),
            target_id: Some(team.id),
            result: AuditEventResult::Success,
            reason: Some("deleted by platform administrator".to_owned()),
            metadata: json!({ "slug": team.slug }),
        },
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

    Ok(())
}

pub(crate) async fn set_quota_plan(
    state: &ControlApiState,
    actor_user_id: Uuid,
    team_id: Uuid,
    plan_id: Option<Uuid>,
) -> Result<team::Model, AppError> {
    const OP: &str = "admin.teams.set_quota_plan";
    let db = crate::infra::http::database(state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;

    let target = teams::get_by_id(db, team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "team not found".to_owned(),
        })?;

    if let Some(plan_id) = plan_id {
        let plan = crate::domain::quotas::get_plan(db, plan_id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        match plan {
            Some(plan) if plan.enabled => {}
            Some(_) => {
                return Err(AppError::Validation {
                    op: OP,
                    message: "quota plan is disabled".to_owned(),
                });
            }
            None => {
                return Err(AppError::Validation {
                    op: OP,
                    message: "quota plan not found".to_owned(),
                });
            }
        }
    }

    use sea_orm::{ActiveModelTrait, ActiveValue::Set};
    let mut active: team::ActiveModel = target.into();
    active.explicit_quota_plan_id = Set(plan_id);
    let team = active
        .update(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(actor_user_id),
            actor_node_id: None,
            team_id: Some(team.id),
            action: "team.quota_plan_overridden".to_owned(),
            target_type: "team".to_owned(),
            target_id: Some(team.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "plan_id": plan_id }),
        },
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

    Ok(team)
}
