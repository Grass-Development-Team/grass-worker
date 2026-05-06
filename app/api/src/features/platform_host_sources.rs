use crate::AppState;
use axum::{
    Extension, Json, Router,
    extract::{Path, rejection::JsonRejection},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use axum_extra::extract::CookieJar;
use chrono::Utc;
use grass_worker_database::entities::platform_host_source;
use grass_worker_database::repository::{
    NewPlatformHostSource, PlatformHostSourceRepository, SeaOrmPlatformHostSourceRepository,
};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PlatformHostSourceKindRequest {
    WildcardStatic,
    DnsManaged,
}

#[derive(Debug, Deserialize)]
struct CreatePlatformHostSourceRequest {
    kind: PlatformHostSourceKindRequest,
    label: String,
    base_domain: String,
    enabled: bool,
    allows_auto_assign: bool,
}

#[derive(Debug, Serialize)]
struct PlatformHostSourceResponse {
    id: Uuid,
    kind: &'static str,
    label: String,
    base_domain: String,
    enabled: bool,
    allows_auto_assign: bool,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<platform_host_source::Model> for PlatformHostSourceResponse {
    fn from(value: platform_host_source::Model) -> Self {
        Self {
            id: value.id,
            kind: source_kind_label(&value.kind),
            label: value.label,
            base_domain: value.base_domain,
            enabled: value.enabled,
            allows_auto_assign: value.allows_auto_assign,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct PlatformHostSourceEnvelope {
    source: PlatformHostSourceResponse,
}

#[derive(Debug, Serialize)]
struct PlatformHostSourcesEnvelope {
    sources: Vec<PlatformHostSourceResponse>,
}

pub fn install_platform_host_source_routes(router: Router, state: AppState) -> Router {
    let source_router = Router::new()
        .route(
            "/api/v1/admin/platform-host-sources",
            get(list_sources).post(create_source),
        )
        .route(
            "/api/v1/admin/platform-host-sources/{id}/enable",
            post(enable_source),
        )
        .route(
            "/api/v1/admin/platform-host-sources/{id}/disable",
            post(disable_source),
        )
        .layer(Extension(state));

    router.merge(source_router)
}

async fn list_sources(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
) -> axum::response::Response {
    let actor = match authenticated_admin(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    if !actor.is_admin {
        return error_response(StatusCode::FORBIDDEN, "forbidden");
    }

    match host_source_repository(state.database.as_ref())
        .list_all()
        .await
    {
        Ok(sources) => Json(PlatformHostSourcesEnvelope {
            sources: sources
                .into_iter()
                .map(PlatformHostSourceResponse::from)
                .collect(),
        })
        .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "list platform host sources failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal platform host source error",
            )
        }
    }
}

async fn create_source(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    payload: Result<Json<CreatePlatformHostSourceRequest>, JsonRejection>,
) -> axum::response::Response {
    let actor = match authenticated_admin(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    if !actor.is_admin {
        return error_response(StatusCode::FORBIDDEN, "forbidden");
    }
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "create platform host source request rejected: invalid payload"
            );
            return error_response(StatusCode::BAD_REQUEST, error.to_string());
        }
    };
    let label = match normalize_label(&payload.label) {
        Ok(label) => label,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error),
    };
    let base_domain = match crate::domain::host::normalize_host(&payload.base_domain) {
        Ok(base_domain) => base_domain,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error.message()),
    };

    match host_source_repository(state.database.as_ref())
        .create(NewPlatformHostSource {
            id: Uuid::new_v4(),
            kind: request_kind_to_model(payload.kind),
            label,
            base_domain,
            enabled: payload.enabled,
            allows_auto_assign: payload.allows_auto_assign,
            created_at: Utc::now(),
        })
        .await
    {
        Ok(source) => (
            StatusCode::CREATED,
            Json(PlatformHostSourceEnvelope {
                source: source.into(),
            }),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "create platform host source failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal platform host source error",
            )
        }
    }
}

async fn enable_source(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> axum::response::Response {
    update_source_enabled(state, jar, id, true).await
}

async fn disable_source(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> axum::response::Response {
    update_source_enabled(state, jar, id, false).await
}

async fn update_source_enabled(
    state: AppState,
    jar: CookieJar,
    raw_id: String,
    enabled: bool,
) -> axum::response::Response {
    let actor = match authenticated_admin(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };
    if !actor.is_admin {
        return error_response(StatusCode::FORBIDDEN, "forbidden");
    }
    let id = match Uuid::parse_str(&raw_id) {
        Ok(id) => id,
        Err(_error) => return error_response(StatusCode::BAD_REQUEST, "invalid host source id"),
    };

    match host_source_repository(state.database.as_ref())
        .set_enabled(id, enabled, Utc::now())
        .await
    {
        Ok(Some(source)) => Json(PlatformHostSourceEnvelope {
            source: source.into(),
        })
        .into_response(),
        Ok(None) => error_response(StatusCode::NOT_FOUND, "host source not found"),
        Err(error) => {
            tracing::error!(error = %error, "update platform host source failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal platform host source error",
            )
        }
    }
}

async fn authenticated_admin(
    state: &AppState,
    jar: CookieJar,
) -> Result<crate::domain::auth::AuthenticatedUser, axum::response::Response> {
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

fn request_kind_to_model(
    value: PlatformHostSourceKindRequest,
) -> platform_host_source::PlatformHostSourceKind {
    match value {
        PlatformHostSourceKindRequest::WildcardStatic => {
            platform_host_source::PlatformHostSourceKind::WildcardStatic
        }
        PlatformHostSourceKindRequest::DnsManaged => {
            platform_host_source::PlatformHostSourceKind::DnsManaged
        }
    }
}

fn source_kind_label(value: &platform_host_source::PlatformHostSourceKind) -> &'static str {
    match value {
        platform_host_source::PlatformHostSourceKind::WildcardStatic => "wildcard_static",
        platform_host_source::PlatformHostSourceKind::DnsManaged => "dns_managed",
    }
}

fn normalize_label(label: &str) -> Result<String, &'static str> {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return Err("label is required");
    }

    Ok(trimmed.to_owned())
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

fn host_source_repository(database: &DatabaseConnection) -> SeaOrmPlatformHostSourceRepository {
    SeaOrmPlatformHostSourceRepository::new(clone_database_connection(database))
}
