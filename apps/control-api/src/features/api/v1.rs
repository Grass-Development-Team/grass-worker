pub mod admin;
pub mod auth;
pub mod me;
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
}
