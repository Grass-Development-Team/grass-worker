use crate::AppState;
use axum::{Extension, Json, Router, http::StatusCode, response::IntoResponse, routing::get};
use axum_extra::extract::CookieJar;
use grass_worker_database::entities::project;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Serialize)]
struct ProjectEnvelope {
    projects: Vec<ProjectSummary>,
}

#[derive(Debug, Serialize)]
struct ProjectSummary {
    id: uuid::Uuid,
    slug: String,
    name: String,
    status: &'static str,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    archived_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub fn install_project_routes(router: Router, state: AppState) -> Router {
    router.merge(
        Router::new()
            .route("/api/v1/projects", get(list_projects))
            .layer(Extension(state)),
    )
}

async fn list_projects(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
) -> axum::response::Response {
    let current_user = match authenticate(&state, &jar).await {
        Ok(current_user) => current_user,
        Err(error) => return auth_error_response(error),
    };

    let mut projects = match project::Entity::find()
        .filter(project::Column::OwnerUserId.eq(current_user.id))
        .all(state.database.as_ref())
        .await
    {
        Ok(projects) => projects,
        Err(error) => {
            tracing::error!(error = %error, user_id = %current_user.id, "project listing failed");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal project error");
        }
    };
    projects.sort_by(|left, right| {
        project_rank(&left.status)
            .cmp(&project_rank(&right.status))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.name.cmp(&right.name))
    });

    Json(ProjectEnvelope {
        projects: projects
            .into_iter()
            .map(|project| ProjectSummary {
                id: project.id,
                slug: project.slug,
                name: project.name,
                status: project_status(&project.status),
                created_at: project.created_at,
                updated_at: project.updated_at,
                archived_at: project.archived_at,
            })
            .collect(),
    })
    .into_response()
}

async fn authenticate(
    state: &AppState,
    jar: &CookieJar,
) -> Result<crate::domain::auth::AuthenticatedUser, crate::domain::auth::AuthError> {
    let Some(session_cookie) = jar.get(crate::domain::auth::SESSION_COOKIE_NAME) else {
        return Err(crate::domain::auth::AuthError::unauthorized(
            "missing session",
        ));
    };

    state
        .auth
        .current_user(state.database.as_ref(), session_cookie.value())
        .await
}

fn project_rank(status: &project::ProjectStatus) -> u8 {
    match status {
        project::ProjectStatus::Active => 0,
        project::ProjectStatus::Archived => 1,
    }
}

fn project_status(status: &project::ProjectStatus) -> &'static str {
    match status {
        project::ProjectStatus::Active => "active",
        project::ProjectStatus::Archived => "archived",
    }
}

fn auth_error_response(error: crate::domain::auth::AuthError) -> axum::response::Response {
    match error.kind() {
        crate::domain::auth::AuthErrorKind::Validation => {
            error_response(StatusCode::BAD_REQUEST, error.message())
        }
        crate::domain::auth::AuthErrorKind::Unauthorized => {
            error_response(StatusCode::UNAUTHORIZED, error.message())
        }
        crate::domain::auth::AuthErrorKind::Internal => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal auth error")
        }
    }
}

fn error_response(status: StatusCode, error: impl Into<String>) -> axum::response::Response {
    (
        status,
        Json(ErrorResponse {
            error: error.into(),
        }),
    )
        .into_response()
}
