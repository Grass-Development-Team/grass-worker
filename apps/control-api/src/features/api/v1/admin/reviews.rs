use axum::{extract::State, response::IntoResponse};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::{
    domain::deployments,
    infra::{
        database::entity::{
            DeploymentEventKind, DeploymentReviewStatus, DeploymentServeStatus, deployment,
            deployment_event, deployment_review, project, team, user,
        },
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/reviews", axum::routing::get(list))
}

/// GET /api/v1/admin/reviews — pending release reviews across all teams,
/// oldest first.
pub async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.reviews.list";
    let db = crate::infra::http::database(&state, OP)?;

    let reviews = deployment_review::Entity::find()
        .filter(deployment_review::Column::Status.eq(DeploymentReviewStatus::Pending))
        .order_by_asc(deployment_review::Column::RequestedAt)
        .limit(200)
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    let deployment_ids: Vec<Uuid> = reviews.iter().map(|review| review.deployment_id).collect();
    let deployments_map: HashMap<Uuid, deployment::Model> = if deployment_ids.is_empty() {
        HashMap::new()
    } else {
        deployment::Entity::find()
            .filter(deployment::Column::Id.is_in(deployment_ids.clone()))
            .filter(deployment::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .into_iter()
            .map(|deployment| (deployment.id, deployment))
            .collect()
    };
    let serve_ready_ids = if deployment_ids.is_empty() {
        HashSet::new()
    } else {
        deployment_event::Entity::find()
            .filter(deployment_event::Column::DeploymentId.is_in(deployment_ids.clone()))
            .filter(deployment_event::Column::Kind.eq(DeploymentEventKind::Serve))
            .all(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .into_iter()
            .filter(|event| {
                event
                    .metadata
                    .get("status")
                    .and_then(|value| value.as_str())
                    == Some("ready")
            })
            .map(|event| event.deployment_id)
            .collect::<HashSet<_>>()
    };

    let project_ids: Vec<Uuid> = deployments_map
        .values()
        .map(|deployment| deployment.project_id)
        .collect();
    let projects_map: HashMap<Uuid, project::Model> = if project_ids.is_empty() {
        HashMap::new()
    } else {
        project::Entity::find()
            .filter(project::Column::Id.is_in(project_ids))
            .all(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .into_iter()
            .map(|project| (project.id, project))
            .collect()
    };

    let team_ids: Vec<Uuid> = deployments_map
        .values()
        .map(|deployment| deployment.team_id)
        .collect();
    let teams_map: HashMap<Uuid, team::Model> = if team_ids.is_empty() {
        HashMap::new()
    } else {
        team::Entity::find()
            .filter(team::Column::Id.is_in(team_ids))
            .all(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .into_iter()
            .map(|team| (team.id, team))
            .collect()
    };

    let user_ids: Vec<Uuid> = deployments_map
        .values()
        .filter_map(|deployment| deployment.triggered_by_user_id)
        .collect();
    let users_map: HashMap<Uuid, user::Model> = if user_ids.is_empty() {
        HashMap::new()
    } else {
        user::Entity::find()
            .filter(user::Column::Id.is_in(user_ids))
            .all(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .into_iter()
            .map(|user| (user.id, user))
            .collect()
    };

    let rows: Vec<ReviewResponse> = reviews
        .iter()
        .filter_map(|review| {
            let deployment = deployments_map.get(&review.deployment_id)?;
            let project = projects_map.get(&deployment.project_id)?;
            let team = teams_map.get(&deployment.team_id);
            let triggered_by = deployment
                .triggered_by_user_id
                .and_then(|id| users_map.get(&id));
            Some(ReviewResponse {
                id: review.id,
                requested_at: review.requested_at,
                deployment: ReviewDeploymentResponse {
                    id: deployment.id,
                    environment: deployments::environment_value(&deployment.environment),
                    build_status: deployments::build_status_value(&deployment.build_status),
                    serve_status: deployments::serve_status_value(&deployment.serve_status),
                    serve_was_ready: serve_ready_ids.contains(&deployment.id),
                    release_status: deployments::release_status_value(&deployment.release_status),
                    source_branch: deployment.source_branch.clone(),
                    commit_hash: deployment.commit_hash.clone(),
                    commit_message: deployment.commit_message.clone(),
                    preview_host: (!matches!(
                        deployment.serve_status,
                        DeploymentServeStatus::Retired
                    ))
                    .then_some(deployment.preview_host.clone())
                    .flatten(),
                    created_at: deployment.created_at,
                },
                project: ReviewProjectResponse {
                    id: project.id,
                    name: project.name.clone(),
                    slug: project.slug.clone(),
                },
                team: team.map(|team| ReviewTeamResponse {
                    id: team.id,
                    name: team.name.clone(),
                    slug: team.slug.clone(),
                }),
                triggered_by: triggered_by.map(|user| ReviewActorResponse {
                    id: user.id,
                    email: user.email.clone(),
                    display_name: user.display_name.clone(),
                }),
            })
        })
        .collect();

    Ok(ok_response(ListResponse {
        total: rows.len(),
        reviews: rows,
    }))
}

#[derive(serde::Serialize)]
struct ListResponse {
    total: usize,
    reviews: Vec<ReviewResponse>,
}
#[derive(serde::Serialize)]
struct ReviewResponse {
    id: Uuid,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    requested_at: time::OffsetDateTime,
    deployment: ReviewDeploymentResponse,
    project: ReviewProjectResponse,
    team: Option<ReviewTeamResponse>,
    triggered_by: Option<ReviewActorResponse>,
}
#[derive(serde::Serialize)]
struct ReviewDeploymentResponse {
    id: Uuid,
    environment: &'static str,
    build_status: &'static str,
    serve_status: &'static str,
    serve_was_ready: bool,
    release_status: &'static str,
    source_branch: Option<String>,
    commit_hash: Option<String>,
    commit_message: Option<String>,
    preview_host: Option<String>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}
#[derive(serde::Serialize)]
struct ReviewProjectResponse {
    id: Uuid,
    name: String,
    slug: String,
}
#[derive(serde::Serialize)]
struct ReviewTeamResponse {
    id: Uuid,
    name: String,
    slug: String,
}
#[derive(serde::Serialize)]
struct ReviewActorResponse {
    id: Uuid,
    email: String,
    display_name: Option<String>,
}
