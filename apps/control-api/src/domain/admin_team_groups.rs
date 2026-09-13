use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::teams,
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, team, team_group},
        error::AppError,
    },
    state::ControlApiState,
};

pub(crate) async fn assign(
    state: &ControlApiState,
    actor_user_id: Uuid,
    team_id: Uuid,
    group_id: Uuid,
) -> Result<team::Model, AppError> {
    const OP: &str = "admin.teams.assign_group";
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
    let group = find_group(db, group_id, OP).await?;

    let mut active: team::ActiveModel = target.into();
    active.group_id = Set(Some(group.id));
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
            action: "team.group_changed".to_owned(),
            target_type: "team".to_owned(),
            target_id: Some(team.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "group_id": group.id, "group_code": group.code }),
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

pub(crate) async fn find_group(
    db: &impl sea_orm::ConnectionTrait,
    group_id: Uuid,
    op: &'static str,
) -> Result<team_group::Model, AppError> {
    team_group::Entity::find()
        .filter(team_group::Column::Id.eq(group_id))
        .filter(team_group::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "team group not found".to_owned(),
        })
}
