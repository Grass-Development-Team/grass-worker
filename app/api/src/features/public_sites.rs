use crate::AppState;
use axum::{
    Extension, Router,
    http::{HeaderMap, StatusCode, Uri, header},
    response::IntoResponse,
    routing::get,
};
use grass_worker_database::repository::{
    ProjectHostBindingRepository, SeaOrmProjectHostBindingRepository,
};
use sea_orm::DatabaseConnection;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct PublicSiteState {
    state: AppState,
    public_dir: PathBuf,
}

pub fn install_public_site_frontend(
    router: Router,
    state: AppState,
    public_dir: PathBuf,
) -> Router {
    Router::new()
        .route("/", get(site_or_frontend).head(site_or_frontend))
        .route("/{*path}", get(site_or_frontend).head(site_or_frontend))
        .layer(Extension(PublicSiteState { state, public_dir }))
        .merge(router)
}

async fn site_or_frontend(
    Extension(state): Extension<PublicSiteState>,
    uri: Uri,
    headers: HeaderMap,
) -> axum::response::Response {
    if uri.path() == "/sites" || uri.path().starts_with("/sites/") {
        return StatusCode::NOT_FOUND.into_response();
    }

    let frontend_fallback =
        || match crate::frontend::resolve_release_asset(&state.public_dir, uri.path()) {
            Ok(Some(asset)) => asset.into_response(),
            Ok(None) => StatusCode::NOT_FOUND.into_response(),
            Err(_error) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };

    let Some(raw_host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return frontend_fallback();
    };
    let normalized_host = match crate::domain::host::normalize_host(raw_host) {
        Ok(host) => host,
        Err(_error) => return frontend_fallback(),
    };

    let site = match state
        .state
        .sites
        .resolve_by_host(state.state.database.as_ref(), &normalized_host)
        .await
    {
        Ok(Some(site)) => site,
        Ok(None) => {
            let binding = match host_binding_repository(state.state.database.as_ref())
                .find_by_host(&normalized_host)
                .await
            {
                Ok(binding) => binding,
                Err(error) => {
                    tracing::error!(error = %error, "public site host lookup failed");
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            };

            return if binding.is_some() {
                StatusCode::NOT_FOUND.into_response()
            } else {
                frontend_fallback()
            };
        }
        Err(error) => {
            tracing::error!(error = ?error, "public site resolution failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    match crate::frontend::resolve_release_asset(Path::new(&site.root_dir), uri.path()) {
        Ok(Some(asset)) => asset.into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_error) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
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
