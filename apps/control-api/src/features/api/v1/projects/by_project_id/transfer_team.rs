use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{
        project_lifecycle::{record_lifecycle_audit, runtime_dimension},
        projects,
        quotas::QuotaDimension,
        teams,
    },
    infra::{
        database::entity::{TeamMemberRole, project},
        error::{AppError, ok_response},
        http::extractors::Session,
        quota::{QuotaCharge, QuotaService},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/transfer-team",
        axum::routing::post(transfer_team),
    )
}

fn project_view(project: &project::Model) -> ProjectResponse {
    ProjectResponse {
        id: project.id,
        team_id: project.team_id,
        slug: project.slug.clone(),
        name: project.name.clone(),
        runtime: projects::runtime_value(&project.runtime),
        repository_url: project.repository_url.clone(),
        default_branch: project.default_branch.clone(),
        install_command: project.install_command.clone(),
        build_command: project.build_command.clone(),
        output_directory: project.output_directory.clone(),
        source_config: project.source_config.clone(),
        build_config: project.build_config.clone(),
        archived_at: project.archived_at,
        deleted_at: project.deleted_at,
        created_at: project.created_at,
        updated_at: project.updated_at,
    }
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
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    access.require_owner(OP)?;
    let db = crate::infra::http::database(&state, OP)?;
    let cache = crate::infra::http::cache(&state, OP)?;

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
    let project = match async {
        let transaction = audits::AuditTransaction::begin(db).await?;
        let project = projects::transfer_team(&transaction, access.project, target_team.id).await?;
        record_lifecycle_audit(
            &transaction,
            session.data.user_id,
            target_team.id,
            "project.transferred",
            project.id,
            json!({ "from_team_id": source_team_id, "to_team_id": target_team.id }),
        )
        .await?;
        transaction.commit().await?;
        Ok::<_, anyhow::Error>(project)
    }
    .await
    {
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

    Ok(ok_response(TransferTeamResponse {
        project: project_view(&project),
    }))
}

#[derive(serde::Serialize)]
struct ProjectResponse {
    id: uuid::Uuid,
    team_id: uuid::Uuid,
    slug: String,
    name: String,
    runtime: &'static str,
    repository_url: Option<String>,
    default_branch: Option<String>,
    install_command: Option<String>,
    build_command: Option<String>,
    output_directory: Option<String>,
    source_config: serde_json::Value,
    build_config: serde_json::Value,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    archived_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    deleted_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    updated_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct TransferTeamResponse {
    project: ProjectResponse,
}
