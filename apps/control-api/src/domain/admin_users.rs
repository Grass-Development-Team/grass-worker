use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::users::{self, UpdateUserParams},
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, PlatformRole, UserStatus},
        error::AppError,
    },
    state::ControlApiState,
};

pub(crate) async fn update(
    state: &ControlApiState,
    actor_user_id: Uuid,
    user_id: Uuid,
    changes: UpdateUserParams,
) -> Result<crate::infra::database::entity::user::Model, AppError> {
    const OP: &str = "admin.users.update";
    let db = crate::infra::http::database(state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;

    let target = users::get_user_by_id(db, user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "user not found".to_owned(),
        })?;

    let UpdateUserParams {
        display_name,
        status,
        platform_role,
    } = changes;
    let demotes_admin = target.platform_role == PlatformRole::Admin
        && matches!(platform_role, Some(PlatformRole::User));
    let disables_user = matches!(status, Some(UserStatus::Disabled));

    if target.id == actor_user_id && (demotes_admin || disables_user) {
        return Err(AppError::Validation {
            op: OP,
            message: "you cannot disable or demote your own account".to_owned(),
        });
    }
    if target.platform_role == PlatformRole::Admin
        && target.status == UserStatus::Active
        && (demotes_admin || disables_user)
    {
        let admins = users::count_active_admins(db)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        if admins <= 1 {
            return Err(AppError::Validation {
                op: OP,
                message: "the platform must keep at least one active administrator".to_owned(),
            });
        }
    }

    let mut changed: Vec<&'static str> = Vec::new();
    if display_name.is_some() {
        changed.push("display_name");
    }
    if status.is_some() {
        changed.push("status");
    }
    if platform_role.is_some() {
        changed.push("platform_role");
    }

    let updated = users::update_user(
        db,
        target,
        UpdateUserParams {
            display_name,
            status,
            platform_role,
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(actor_user_id),
            actor_node_id: None,
            team_id: None,
            action: "user.updated".to_owned(),
            target_type: "user".to_owned(),
            target_id: Some(updated.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "changed": changed }),
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

    Ok(updated)
}
