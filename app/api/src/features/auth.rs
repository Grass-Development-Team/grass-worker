use crate::AppState;
use axum::{
    Extension, Router,
    http::StatusCode,
    routing::{get, post},
};
use axum_extra::extract::CookieJar;

pub fn install_auth_routes(router: Router, state: AppState) -> Router {
    let auth_router = Router::new()
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/me", get(current_user))
        .layer(Extension(state));

    router.merge(auth_router)
}

async fn login() -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}

async fn current_user() -> StatusCode {
    StatusCode::UNAUTHORIZED
}

async fn logout(jar: CookieJar) -> (CookieJar, StatusCode) {
    (crate::adapters::auth::clear_session_cookie(jar), StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::get,
    };
    use sea_orm::{DatabaseBackend, MockDatabase};
    use tower::ServiceExt;

    #[tokio::test]
    async fn logout_sets_path_aware_session_clear_cookie() {
        let app = install_auth_routes(
            Router::new(),
            AppState::new(MockDatabase::new(DatabaseBackend::Postgres).into_connection()),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/logout")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let set_cookie = response
            .headers()
            .get(axum::http::header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        assert!(set_cookie.contains("gw_session="));
        assert!(set_cookie.contains("Path=/"));
    }

    #[tokio::test]
    async fn install_auth_routes_does_not_leak_app_state_layer_to_existing_routes() {
        async fn pre_existing_route(_state: Extension<AppState>) -> StatusCode {
            StatusCode::OK
        }

        let base_router = Router::new().route("/pre-existing", get(pre_existing_route));
        let app = install_auth_routes(
            base_router,
            AppState::new(MockDatabase::new(DatabaseBackend::Postgres).into_connection()),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/pre-existing")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
