pub mod admin;
pub mod auth;
pub mod internal;
pub mod me;
pub mod preview_auth;
pub mod projects;
pub mod setup;
pub mod teams;

use axum::{Router, middleware, routing::get};

use crate::{
    infra::http::extractors::PlatformAdmin,
    infra::http::middlewares::setup_mode::{require_ready_mode, require_setup_mode},
    state::ControlApiState,
};

pub fn router(state: ControlApiState) -> Router<ControlApiState> {
    let administration = admin::router()
        .route_layer(middleware::from_extractor_with_state::<PlatformAdmin, _>(
            state.clone(),
        ));

    Router::new()
        .nest(
            "/setup",
            setup::router().layer(middleware::from_fn_with_state(
                state.clone(),
                require_setup_mode,
            )),
        )
        .nest(
            "/auth",
            auth::router().layer(middleware::from_fn_with_state(
                state.clone(),
                require_ready_mode,
            )),
        )
        .route(
            "/preview/authorize",
            get(preview_auth::authorize)
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    require_ready_mode,
                ))
                .layer(middleware::map_response(
                    preview_auth::browser_authorization_headers,
                )),
        )
        .route(
            "/me",
            get(me::handler).layer(middleware::from_fn_with_state(
                state.clone(),
                require_ready_mode,
            )),
        )
        .nest("/admin", administration)
        .merge(teams::router().layer(middleware::from_fn_with_state(
            state.clone(),
            require_ready_mode,
        )))
        .merge(projects::router().layer(middleware::from_fn_with_state(
            state.clone(),
            require_ready_mode,
        )))
        .nest(
            "/internal",
            internal::router(state.clone())
                // Artifact uploads carry whole build outputs; quota enforces
                // the real per-team limit.
                .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024 * 1024))
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    require_ready_mode,
                )),
        )
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    use crate::infra::config::ControlApiConfig;

    use super::*;

    #[tokio::test]
    async fn administration_routes_require_authentication() {
        let state = ControlApiState::new(ControlApiConfig::default(), "config.toml");
        let response = router(state.clone())
            .with_state(state)
            .oneshot(
                Request::builder()
                    .uri("/admin/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn preview_authorization_responses_are_not_cached_or_referred() {
        let state = ControlApiState::new(ControlApiConfig::default(), "config.toml");
        let response = router(state.clone())
            .with_state(state)
            .oneshot(
                Request::builder()
                    .uri("/preview/authorize?state=opaque")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.headers()[axum::http::header::CACHE_CONTROL],
            "no-store"
        );
        assert_eq!(
            response.headers()[axum::http::header::REFERRER_POLICY],
            "no-referrer"
        );
    }

    #[tokio::test]
    async fn internal_node_routes_require_a_bearer_token() {
        let state = ControlApiState::new(ControlApiConfig::default(), "config.toml");
        let router = router(state.clone()).with_state(state);

        // Without a database the ready-mode gate rejects first; with one the
        // node auth middleware rejects. Either way no unauthenticated
        // request may ever succeed.
        for uri in [
            "/internal/serve/resolve-host?host=demo.grass.test",
            "/internal/log-stream",
        ] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert!(
                response.status().is_client_error(),
                "{uri} must reject requests without a node token"
            );
        }

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/internal/nodes/heartbeat")
                    .header("authorization", "Bearer wrong-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"active_builds":0}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            !response.status().is_success(),
            "an invalid node token must never authenticate"
        );
    }

    #[tokio::test]
    async fn admin_governance_routes_are_guarded() {
        let state = ControlApiState::new(ControlApiConfig::default(), "config.toml");
        let router = router(state.clone()).with_state(state);

        for uri in [
            "/admin/audit-events",
            "/admin/team-groups",
            "/admin/quota-plans",
            "/admin/host-sources",
            "/admin/nodes",
        ] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                axum::http::StatusCode::UNAUTHORIZED,
                "{uri} must require an authenticated platform administrator"
            );
        }
    }
}
