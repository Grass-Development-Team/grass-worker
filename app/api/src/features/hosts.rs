use crate::AppState;
use axum::{
    Extension, Json, Router,
    extract::{Path, rejection::JsonRejection},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
};
use axum_extra::extract::CookieJar;
use chrono::Utc;
use grass_worker_database::entities::project_host_binding;
use grass_worker_database::repository::{
    ProjectHostBindingRepository, SeaOrmProjectHostBindingRepository,
};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Deserialize)]
struct CreateHostRequest {
    host: String,
    #[serde(default)]
    source_id: Option<Uuid>,
    #[serde(default)]
    is_primary: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ProjectHostBindingResponse {
    id: Uuid,
    project_id: Uuid,
    source_id: Option<Uuid>,
    host: String,
    is_primary: bool,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<project_host_binding::Model> for ProjectHostBindingResponse {
    fn from(value: project_host_binding::Model) -> Self {
        Self {
            id: value.id,
            project_id: value.project_id,
            source_id: value.source_id,
            host: value.host,
            is_primary: value.is_primary,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct HostEnvelope {
    host: ProjectHostBindingResponse,
}

#[derive(Debug, Serialize)]
struct HostsEnvelope {
    hosts: Vec<ProjectHostBindingResponse>,
}

pub fn install_host_routes(router: Router, state: AppState) -> Router {
    let host_router = Router::new()
        .route(
            "/api/v1/projects/{id}/hosts",
            get(list_hosts).post(create_host),
        )
        .route(
            "/api/v1/projects/{id}/hosts/{binding_id}/primary",
            post(set_primary_host),
        )
        .route(
            "/api/v1/projects/{id}/hosts/{binding_id}",
            delete(delete_host),
        )
        .layer(Extension(state));

    router.merge(host_router)
}

async fn list_hosts(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let project_id = match parse_project_id(&id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    if let Err(error) = state
        .projects
        .get(state.database.as_ref(), &actor, project_id)
        .await
    {
        return project_error_response(error);
    }

    match host_binding_repository(state.database.as_ref())
        .list_by_project(project_id)
        .await
    {
        Ok(hosts) => Json(HostsEnvelope {
            hosts: hosts
                .into_iter()
                .map(ProjectHostBindingResponse::from)
                .collect(),
        })
        .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "list hosts database operation failed");
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal host error")
        }
    }
}

async fn create_host(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    payload: Result<Json<CreateHostRequest>, JsonRejection>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let project_id = match parse_project_id(&id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(error = %error, "create host request rejected: invalid payload");
            return error_response(StatusCode::BAD_REQUEST, error.to_string());
        }
    };

    match state
        .hosts
        .create_for_project(
            state.database.as_ref(),
            &actor,
            project_id,
            crate::domain::host::CreateProjectHostInput {
                source_id: payload.source_id,
                host: payload.host,
                is_primary: payload.is_primary,
            },
        )
        .await
    {
        Ok(host) => (
            StatusCode::CREATED,
            Json(HostEnvelope { host: host.into() }),
        )
            .into_response(),
        Err(error) => host_error_response(error),
    }
}

async fn set_primary_host(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path((id, binding_id)): Path<(String, String)>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let project_id = match parse_project_id(&id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    let binding_id = match parse_binding_id(&binding_id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    if let Err(error) = state
        .projects
        .get(state.database.as_ref(), &actor, project_id)
        .await
    {
        return project_error_response(error);
    }

    let repository = host_binding_repository(state.database.as_ref());
    let binding = match repository.find_by_id(binding_id).await {
        Ok(Some(binding)) if binding.project_id == project_id => binding,
        Ok(Some(_)) | Ok(None) => return error_response(StatusCode::NOT_FOUND, "host not found"),
        Err(error) => {
            tracing::error!(error = %error, "set primary host database lookup failed");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal host error");
        }
    };

    match repository.set_primary(binding.id, Utc::now()).await {
        Ok(Some(host)) => Json(HostEnvelope { host: host.into() }).into_response(),
        Ok(None) => error_response(StatusCode::NOT_FOUND, "host not found"),
        Err(error) => {
            tracing::error!(error = %error, "set primary host database update failed");
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal host error")
        }
    }
}

async fn delete_host(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path((id, binding_id)): Path<(String, String)>,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    let project_id = match parse_project_id(&id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    let binding_id = match parse_binding_id(&binding_id) {
        Ok(id) => id,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    if let Err(error) = state
        .projects
        .get(state.database.as_ref(), &actor, project_id)
        .await
    {
        return project_error_response(error);
    }

    let repository = host_binding_repository(state.database.as_ref());
    let binding = match repository.find_by_id(binding_id).await {
        Ok(Some(binding)) if binding.project_id == project_id => binding,
        Ok(Some(_)) | Ok(None) => return error_response(StatusCode::NOT_FOUND, "host not found"),
        Err(error) => {
            tracing::error!(error = %error, "delete host database lookup failed");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal host error");
        }
    };

    match repository.delete(binding.id).await {
        Ok(true) => {}
        Ok(false) => return error_response(StatusCode::NOT_FOUND, "host not found"),
        Err(error) => {
            tracing::error!(error = %error, "delete host database operation failed");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal host error");
        }
    }

    if binding.is_primary {
        let remaining = match repository.list_by_project(project_id).await {
            Ok(remaining) => remaining,
            Err(error) => {
                tracing::error!(error = %error, "list remaining hosts after delete failed");
                return error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal host error");
            }
        };

        if let Some(next_primary) = remaining.first() {
            if !next_primary.is_primary {
                match repository.set_primary(next_primary.id, Utc::now()).await {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        return error_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "internal host error",
                        );
                    }
                    Err(error) => {
                        tracing::error!(error = %error, "promote replacement primary host failed");
                        return error_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "internal host error",
                        );
                    }
                }
            }
        }
    }

    StatusCode::NO_CONTENT.into_response()
}

fn authenticated_user(
    state: &AppState,
    jar: CookieJar,
) -> impl std::future::Future<
    Output = Result<crate::domain::auth::AuthenticatedUser, axum::response::Response>,
> {
    async move {
        let Some(session_cookie) = jar.get(crate::domain::auth::SESSION_COOKIE_NAME) else {
            return Err(auth_error_response(
                crate::domain::auth::AuthError::unauthorized("missing session"),
            ));
        };

        match state
            .auth
            .current_user(state.database.as_ref(), session_cookie.value())
            .await
        {
            Ok(user) => Ok(user),
            Err(error) => Err(auth_error_response(error)),
        }
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

fn host_error_response(error: crate::domain::host::HostError) -> axum::response::Response {
    match error.kind() {
        crate::domain::host::HostErrorKind::Validation => {
            error_response(StatusCode::BAD_REQUEST, error.message())
        }
        crate::domain::host::HostErrorKind::NotFound => {
            error_response(StatusCode::NOT_FOUND, error.message())
        }
        crate::domain::host::HostErrorKind::Forbidden => {
            error_response(StatusCode::FORBIDDEN, error.message())
        }
        crate::domain::host::HostErrorKind::Conflict => {
            error_response(StatusCode::CONFLICT, error.message())
        }
        crate::domain::host::HostErrorKind::Internal => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal host error")
        }
    }
}

fn project_error_response(error: crate::domain::project::ProjectError) -> axum::response::Response {
    match error.kind() {
        crate::domain::project::ProjectErrorKind::Validation => {
            error_response(StatusCode::BAD_REQUEST, error.message())
        }
        crate::domain::project::ProjectErrorKind::NotFound => {
            error_response(StatusCode::NOT_FOUND, error.message())
        }
        crate::domain::project::ProjectErrorKind::Forbidden => {
            error_response(StatusCode::FORBIDDEN, error.message())
        }
        crate::domain::project::ProjectErrorKind::Conflict => {
            error_response(StatusCode::CONFLICT, error.message())
        }
        crate::domain::project::ProjectErrorKind::Internal => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal project error")
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

fn parse_project_id(raw: &str) -> Result<Uuid, &'static str> {
    Uuid::parse_str(raw).map_err(|_error| "invalid project id")
}

fn parse_binding_id(raw: &str) -> Result<Uuid, &'static str> {
    Uuid::parse_str(raw).map_err(|_error| "invalid host binding id")
}

fn clone_database_connection(database: &DatabaseConnection) -> DatabaseConnection {
    match database {
        DatabaseConnection::SqlxPostgresPoolConnection(connection) => {
            DatabaseConnection::SqlxPostgresPoolConnection(connection.clone())
        }
        DatabaseConnection::MockDatabaseConnection(connection) => {
            DatabaseConnection::MockDatabaseConnection(connection.clone())
        }
        DatabaseConnection::Disconnected => DatabaseConnection::Disconnected,
    }
}

fn host_binding_repository(database: &DatabaseConnection) -> SeaOrmProjectHostBindingRepository {
    SeaOrmProjectHostBindingRepository::new(clone_database_connection(database))
}
