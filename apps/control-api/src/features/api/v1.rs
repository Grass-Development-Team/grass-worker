pub mod admin;
pub mod announcements;
pub mod auth;
pub mod internal;
pub mod me;
pub mod notifications;
pub mod preview_auth;
pub mod projects;
pub mod setup;
pub mod site_config;
pub mod teams;

use axum::{
    Router, middleware,
    routing::{get, post},
};

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
        .route("/site-config", get(site_config::get))
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
            get(me::handler)
                .patch(me::update)
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    require_ready_mode,
                )),
        )
        .route(
            "/me/password",
            post(auth::password::change).layer(middleware::from_fn_with_state(
                state.clone(),
                require_ready_mode,
            )),
        )
        .route(
            "/me/security",
            get(auth::mfa::security).layer(middleware::from_fn_with_state(
                state.clone(),
                require_ready_mode,
            )),
        )
        .route(
            "/me/mfa/totp/start",
            post(auth::mfa::account_totp_start).layer(middleware::from_fn_with_state(
                state.clone(),
                require_ready_mode,
            )),
        )
        .route(
            "/me/mfa/email/start",
            post(auth::mfa::account_email_start).layer(middleware::from_fn_with_state(
                state.clone(),
                require_ready_mode,
            )),
        )
        .route(
            "/me/mfa/{factor_id}/confirm",
            post(auth::mfa::account_confirm).layer(middleware::from_fn_with_state(
                state.clone(),
                require_ready_mode,
            )),
        )
        .route(
            "/me/mfa/{factor_id}",
            axum::routing::delete(auth::mfa::account_delete).layer(middleware::from_fn_with_state(
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
        .merge(
            announcements::router().layer(middleware::from_fn_with_state(
                state.clone(),
                require_ready_mode,
            )),
        )
        .merge(
            notifications::router().layer(middleware::from_fn_with_state(
                state.clone(),
                require_ready_mode,
            )),
        )
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
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use sea_orm::MockDatabase;
    use time::OffsetDateTime;
    use tower::ServiceExt;
    use uuid::Uuid;

    use crate::infra::{
        config::ControlApiConfig,
        database::entity::{SystemSettingValueKind, system_setting},
    };

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
    async fn notification_routes_require_authentication() {
        let setting = system_setting::Model {
            id: Uuid::now_v7(),
            key: "setup.finished".to_owned(),
            value_kind: SystemSettingValueKind::Boolean,
            value: serde_json::json!(true),
            is_secret: false,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        };
        let logo_setting = system_setting::Model {
            id: Uuid::now_v7(),
            key: "site.logo_url".to_owned(),
            value_kind: SystemSettingValueKind::String,
            value: serde_json::json!("/assets/acme-logo.svg"),
            is_secret: false,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        };
        let database = MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[setting]])
            .append_query_results([[logo_setting]])
            .into_connection();
        let state = ControlApiState::new(ControlApiConfig::default(), "config.toml");
        assert!(state.database.set(database).is_ok());
        let response = router(state.clone())
            .with_state(state)
            .oneshot(
                Request::builder()
                    .uri("/notifications")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn announcement_routes_require_authentication() {
        let setting = system_setting::Model {
            id: Uuid::now_v7(),
            key: "setup.finished".to_owned(),
            value_kind: SystemSettingValueKind::Boolean,
            value: serde_json::json!(true),
            is_secret: false,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        };
        let database = MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[setting]])
            .into_connection();
        let state = ControlApiState::new(ControlApiConfig::default(), "config.toml");
        assert!(state.database.set(database).is_ok());
        let response = router(state.clone())
            .with_state(state)
            .oneshot(
                Request::builder()
                    .uri("/announcements")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn site_config_is_public_and_uses_the_configured_name() {
        let setting = system_setting::Model {
            id: Uuid::now_v7(),
            key: "site.name".to_owned(),
            value_kind: SystemSettingValueKind::String,
            value: serde_json::json!("Acme Deploy"),
            is_secret: false,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        };
        let logo_setting = system_setting::Model {
            id: Uuid::now_v7(),
            key: "site.logo_url".to_owned(),
            value_kind: SystemSettingValueKind::String,
            value: serde_json::json!("/assets/acme-logo.svg"),
            is_secret: false,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        };
        let database = MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[setting]])
            .append_query_results([[logo_setting]])
            .into_connection();
        let state = ControlApiState::new(ControlApiConfig::default(), "config.toml");
        assert!(state.database.set(database).is_ok());

        let response = router(state.clone())
            .with_state(state)
            .oneshot(
                Request::builder()
                    .uri("/site-config")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["data"]["site_name"], "Acme Deploy");
        assert_eq!(body["data"]["logo_url"], "/assets/acme-logo.svg");
        assert_eq!(body["data"]["version"], env!("CARGO_PKG_VERSION"));
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
            "/admin/cleanup/audit-events",
            "/admin/cleanup/build-logs",
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
