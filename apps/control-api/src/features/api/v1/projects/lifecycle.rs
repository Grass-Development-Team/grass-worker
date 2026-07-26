use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        projects,
        quotas::QuotaDimension,
        teams,
    },
    infra::{
        database::entity::{AuditEventResult, ProjectRuntime, TeamMemberRole},
        error::{AppError, ok_response},
        http::extractors::Session,
        quota::{QuotaCharge, QuotaService},
    },
    state::ControlApiState,
};

async fn record_lifecycle_audit(
    state: &ControlApiState,
    actor: Uuid,
    action: &str,
    project_id: Uuid,
    metadata: serde_json::Value,
) {
    if let Some(db) = state.try_database() {
        let _ = audits::create_audit_event(
            db,
            CreateAuditEventParams {
                actor_user_id: Some(actor),
                action: action.to_owned(),
                target_type: "project".to_owned(),
                target_id: Some(project_id),
                result: AuditEventResult::Success,
                reason: None,
                metadata,
            },
        )
        .await;
    }
}

fn runtime_dimension(runtime: &ProjectRuntime) -> QuotaDimension {
    match runtime {
        ProjectRuntime::Ssr => QuotaDimension::ProjectsSsr,
        _ => QuotaDimension::ProjectsStatic,
    }
}

/// POST /api/v1/projects/{project_id}/archive
pub async fn archive(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.archive";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_admin(OP)?;
    let db = super::database(&state, OP)?;

    let project = projects::set_archived(db, access.project, true)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_lifecycle_audit(
        &state,
        session.data.user_id,
        "project.archived",
        project.id,
        json!({}),
    )
    .await;

    Ok(ok_response(
        json!({ "project": super::project_view(&project) }),
    ))
}

/// POST /api/v1/projects/{project_id}/unarchive
pub async fn unarchive(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.unarchive";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_admin(OP)?;
    let db = super::database(&state, OP)?;

    let project = projects::set_archived(db, access.project, false)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_lifecycle_audit(
        &state,
        session.data.user_id,
        "project.unarchived",
        project.id,
        json!({}),
    )
    .await;

    Ok(ok_response(
        json!({ "project": super::project_view(&project) }),
    ))
}

/// POST /api/v1/projects/{project_id}/delete — soft delete.
pub async fn delete(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.delete";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_admin(OP)?;
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;

    let runtime = access.project.runtime.clone();
    let team_id = access.project.team_id;
    let project = projects::soft_delete(db, access.project)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    QuotaService::new(db, cache)
        .release(
            OP,
            team_id,
            &[
                QuotaCharge::one(QuotaDimension::Projects),
                QuotaCharge::one(runtime_dimension(&runtime)),
            ],
            "project",
            Some(project.id),
        )
        .await?;
    record_lifecycle_audit(
        &state,
        session.data.user_id,
        "project.deleted",
        project.id,
        json!({}),
    )
    .await;

    Ok(ok_response(
        json!({ "project": super::project_view(&project) }),
    ))
}

/// POST /api/v1/projects/{project_id}/restore
pub async fn restore(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.restore";
    let access = super::project_access(&state, &session, project_id, true, OP).await?;
    access.require_admin(OP)?;
    if access.project.deleted_at.is_none() {
        return Err(AppError::Conflict {
            op: OP,
            message: "project is not deleted".to_owned(),
        });
    }
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;

    // Restoring re-consumes project quota; deny restore when the team is
    // already at its limit.
    let quota = QuotaService::new(db, cache);
    let reservation = quota
        .reserve(
            OP,
            &access.team,
            Some(session.data.user_id),
            &[
                QuotaCharge::one(QuotaDimension::Projects),
                QuotaCharge::one(runtime_dimension(&access.project.runtime)),
            ],
        )
        .await?;

    let project = match projects::restore(db, access.project).await {
        Ok(project) => project,
        Err(source) => {
            quota.rollback(reservation).await;
            return Err(AppError::Infrastructure { op: OP, source });
        }
    };
    quota
        .commit(OP, reservation, "project", Some(project.id))
        .await?;
    record_lifecycle_audit(
        &state,
        session.data.user_id,
        "project.restored",
        project.id,
        json!({}),
    )
    .await;

    Ok(ok_response(
        json!({ "project": super::project_view(&project) }),
    ))
}

#[derive(Deserialize)]
pub struct TransferTeamRequest {
    pub team_id: Uuid,
}

/// POST /api/v1/projects/{project_id}/transfer-team
pub async fn transfer_team(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
    Json(body): Json<TransferTeamRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.transfer_team";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_owner(OP)?;
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;

    if body.team_id == access.project.team_id {
        return Err(AppError::Validation {
            op: OP,
            message: "project already belongs to this team".to_owned(),
        });
    }

    let target_team = teams::get_by_id(db, body.team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "target team not found".to_owned(),
        })?;
    let target_role = teams::member_role(db, target_team.id, session.data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::Forbidden {
            op: OP,
            message: "not a member of the target team".to_owned(),
        })?;
    if !matches!(target_role, TeamMemberRole::Owner | TeamMemberRole::Admin) {
        return Err(AppError::Forbidden {
            op: OP,
            message: "admin role required in the target team".to_owned(),
        });
    }

    let charges = [
        QuotaCharge::one(QuotaDimension::Projects),
        QuotaCharge::one(runtime_dimension(&access.project.runtime)),
    ];
    let quota = QuotaService::new(db, cache);
    let reservation = quota
        .reserve(OP, &target_team, Some(session.data.user_id), &charges)
        .await?;

    let source_team_id = access.project.team_id;
    let project = match projects::transfer_team(db, access.project, target_team.id).await {
        Ok(project) => project,
        Err(source) => {
            quota.rollback(reservation).await;
            return Err(AppError::Infrastructure { op: OP, source });
        }
    };
    quota
        .commit(OP, reservation, "project", Some(project.id))
        .await?;
    quota
        .release(OP, source_team_id, &charges, "project", Some(project.id))
        .await?;
    record_lifecycle_audit(
        &state,
        session.data.user_id,
        "project.transferred",
        project.id,
        json!({ "from_team_id": source_team_id, "to_team_id": target_team.id }),
    )
    .await;

    Ok(ok_response(
        json!({ "project": super::project_view(&project) }),
    ))
}

/// POST /api/v1/projects/{project_id}/hard-delete
pub async fn hard_delete(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hard_delete";
    let access = super::project_access(&state, &session, project_id, true, OP).await?;
    access.require_owner(OP)?;
    if access.project.deleted_at.is_none() {
        return Err(AppError::Conflict {
            op: OP,
            message: "project must be soft deleted before it can be hard deleted".to_owned(),
        });
    }
    let db = super::database(&state, OP)?;

    projects::hard_delete(db, access.project.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_lifecycle_audit(
        &state,
        session.data.user_id,
        "project.hard_deleted",
        project_id,
        json!({}),
    )
    .await;

    Ok(ok_response(json!({ "ok": true })))
}
