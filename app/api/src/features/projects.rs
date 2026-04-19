use crate::AppState;
use axum::{
    Extension, Json, Router, extract::Path, extract::rejection::JsonRejection, http::StatusCode,
    response::IntoResponse, routing::get,
};
use axum_extra::extract::CookieJar;
use grass_worker_database::entities::project;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct CreateProjectRequest {
    name: String,
    slug: String,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Serialize)]
struct ProjectEnvelope {
    projects: Vec<ProjectSummary>,
}

#[derive(Debug, Serialize)]
struct ProjectRecordEnvelope {
    project: ProjectSummary,
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
            .route("/api/v1/projects", get(list_projects).post(create_project))
            .route("/api/v1/projects/{project_id}", get(get_project))
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

    let projects = match list_owned_projects(&state, current_user.id).await {
        Ok(projects) => projects,
        Err(error) => {
            tracing::error!(error = %error, user_id = %current_user.id, "project listing failed");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal project error");
        }
    };

    Json(ProjectEnvelope { projects }).into_response()
}

async fn create_project(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    payload: Result<Json<CreateProjectRequest>, JsonRejection>,
) -> axum::response::Response {
    let current_user = match authenticate(&state, &jar).await {
        Ok(current_user) => current_user,
        Err(error) => return auth_error_response(error),
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(error = %error, "create project request rejected: invalid payload");
            return error_response(StatusCode::BAD_REQUEST, error.to_string());
        }
    };

    let name = match normalize_name(&payload.name) {
        Ok(name) => name,
        Err(message) => return error_response(StatusCode::BAD_REQUEST, message),
    };
    let slug = match normalize_slug(&payload.slug) {
        Ok(slug) => slug,
        Err(message) => return error_response(StatusCode::BAD_REQUEST, message),
    };

    match project::Entity::find()
        .filter(project::Column::Slug.eq(slug.clone()))
        .one(state.database.as_ref())
        .await
    {
        Ok(Some(_)) => {
            return error_response(StatusCode::CONFLICT, "project slug already exists");
        }
        Ok(None) => {}
        Err(error) => {
            tracing::error!(error = %error, slug, "project duplicate check failed");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal project error");
        }
    }

    let created_at = chrono::Utc::now();
    let model = project::Model {
        id: uuid::Uuid::new_v4(),
        owner_user_id: current_user.id,
        slug,
        name,
        status: project::ProjectStatus::Active,
        created_at,
        updated_at: created_at,
        archived_at: None,
    };

    match project::Entity::insert(project::ActiveModel {
        id: Set(model.id),
        owner_user_id: Set(model.owner_user_id),
        slug: Set(model.slug.clone()),
        name: Set(model.name.clone()),
        status: Set(model.status.clone()),
        created_at: Set(model.created_at),
        updated_at: Set(model.updated_at),
        archived_at: Set(model.archived_at),
    })
    .exec_without_returning(state.database.as_ref())
    .await
    {
        Ok(_) => (
            StatusCode::CREATED,
            Json(ProjectRecordEnvelope {
                project: map_project(model),
            }),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(error = %error, user_id = %current_user.id, "project creation failed");
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal project error")
        }
    }
}

async fn get_project(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path(project_id): Path<uuid::Uuid>,
) -> axum::response::Response {
    let current_user = match authenticate(&state, &jar).await {
        Ok(current_user) => current_user,
        Err(error) => return auth_error_response(error),
    };

    match project::Entity::find_by_id(project_id)
        .filter(project::Column::OwnerUserId.eq(current_user.id))
        .one(state.database.as_ref())
        .await
    {
        Ok(Some(project)) => Json(ProjectRecordEnvelope {
            project: map_project(project),
        })
        .into_response(),
        Ok(None) => error_response(StatusCode::NOT_FOUND, "project not found"),
        Err(error) => {
            tracing::error!(error = %error, project_id = %project_id, "project lookup failed");
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal project error")
        }
    }
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

async fn list_owned_projects(
    state: &AppState,
    owner_user_id: uuid::Uuid,
) -> Result<Vec<ProjectSummary>, sea_orm::DbErr> {
    let mut projects = project::Entity::find()
        .filter(project::Column::OwnerUserId.eq(owner_user_id))
        .all(state.database.as_ref())
        .await?;
    projects.sort_by(|left, right| {
        project_rank(&left.status)
            .cmp(&project_rank(&right.status))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(projects.into_iter().map(map_project).collect())
}

fn map_project(project: project::Model) -> ProjectSummary {
    ProjectSummary {
        id: project.id,
        slug: project.slug,
        name: project.name,
        status: project_status(&project.status),
        created_at: project.created_at,
        updated_at: project.updated_at,
        archived_at: project.archived_at,
    }
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

fn normalize_name(value: &str) -> Result<String, &'static str> {
    let name = value.trim();
    if name.is_empty() {
        return Err("project name is required");
    }

    Ok(name.to_owned())
}

fn normalize_slug(value: &str) -> Result<String, &'static str> {
    let slug = value.trim().to_ascii_lowercase();
    if slug.is_empty() {
        return Err("project slug is required");
    }

    let bytes = slug.as_bytes();
    if bytes.first() == Some(&b'-') || bytes.last() == Some(&b'-') || slug.contains("--") {
        return Err("project slug must use lowercase letters, numbers, and single hyphens");
    }
    if !slug
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err("project slug must use lowercase letters, numbers, and single hyphens");
    }

    Ok(slug)
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
