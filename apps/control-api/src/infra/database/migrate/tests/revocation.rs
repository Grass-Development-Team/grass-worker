use anyhow::{Context, ensure};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sea_orm_migration::MigratorTrait;
use uuid::Uuid;

use super::super::{MIGRATION_TEST_LOCK, Migrator};
use super::support::{
    PostgresMigrationDatabase, assert_migration_tracking, column, query_column_shapes,
};

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL and disposable schema permission"]
async fn postgres_auth_version_shape_and_password_revocation() -> anyhow::Result<()> {
    postgres_account_revocation(grass_cache::CacheStore::Moka(
        grass_cache::MokaCache::connect(),
    ))
    .await
}

#[tokio::test]
#[ignore = "requires GRASS_TEST_DATABASE_URL, GRASS_TEST_REDIS_URL and disposable schema permission"]
async fn postgres_redis_auth_version_shape_and_password_revocation() -> anyhow::Result<()> {
    postgres_account_revocation(grass_cache::CacheStore::Redis(
        grass_cache::RedisCache::connect(&std::env::var("GRASS_TEST_REDIS_URL")?).await?,
    ))
    .await
}

async fn postgres_account_revocation(cache_store: grass_cache::CacheStore) -> anyhow::Result<()> {
    use std::time::Duration;

    use axum::{Router, body::Body, http::Request, middleware, routing::get};
    use grass_cache::Cache;
    use tower::ServiceExt;

    use crate::{
        domain::{authentication, users},
        infra::{
            config::ControlApiConfig,
            database::entity::{AuthTokenKind, PlatformRole, UserStatus},
            http::{extractors::Session, middlewares::session},
        },
        state::ControlApiState,
    };

    let _guard = MIGRATION_TEST_LOCK.lock().await;
    let database =
        PostgresMigrationDatabase::start(&std::env::var("GRASS_TEST_DATABASE_URL")?).await?;
    let result: anyhow::Result<()> = async {
        let db = &database.db;
        Migrator::up(db, None).await?;
        assert_migration_tracking(db, 35).await?;
        let shapes = query_column_shapes(
            db,
            "SELECT column_name, udt_name, is_nullable, column_default FROM \
                    information_schema.columns WHERE table_schema = current_schema() AND \
                    table_name = 'users' AND column_name = 'auth_version'",
        )
        .await?;
        ensure!(
            shapes == vec![column("auth_version", "int8", "NO", Some("1"))],
            "incorrect authentication version column shape"
        );
        let constraint = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT pg_get_constraintdef(oid) AS definition FROM pg_constraint WHERE \
                    conrelid = 'users'::regclass AND conname = 'users_auth_version_check'",
            ))
            .await?
            .unwrap();
        ensure!(
            constraint
                .try_get::<String>("", "definition")?
                .contains("auth_version > 0")
        );
        let user = users::create_user(
            db,
            users::CreateUserParams {
                email: format!("revocation-{}@example.test", Uuid::now_v7()),
                display_name: None,
                password_hash: Some(grass_crypto::hash_password("Original-password-123!")?),
                platform_role: PlatformRole::Admin,
                email_verified_at: Some(time::OffsetDateTime::now_utc()),
            },
        )
        .await?;
        ensure!(user.auth_version == 1);
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        state.database.set(db.clone()).ok().unwrap();
        state.cache.set(cache_store).ok().unwrap();
        async fn protected(_session: Session) -> &'static str {
            "allowed"
        }
        async fn admin(_admin: crate::infra::http::extractors::PlatformAdmin) -> &'static str {
            "allowed"
        }
        let app = Router::new()
            .route("/protected", get(protected).post(protected))
            .route("/admin", get(admin))
            .nest(
                "/api/v1/auth",
                crate::features::api::v1::auth::login::router()
                    .merge(crate::features::api::v1::auth::password::reset::router()),
            )
            .nest("/api/v1", crate::features::api::v1::me::password::router())
            .nest(
                "/api/v1/admin",
                crate::features::api::v1::admin::users::by_user_id::reset_password::router(),
            )
            .layer(middleware::from_fn_with_state(
                state.clone(),
                session::session_middleware,
            ))
            .with_state(state.clone());
        let cache = state.try_cache().unwrap();
        let mut current_password = "Original-password-123!";
        for (flow, next_password) in [
            ("change", "Changed-password-123!"),
            ("reset", "Reset-password-123!"),
            ("admin", "Admin-reset-password-123!"),
        ] {
            let current = users::get_user_by_id(db, user.id).await?.unwrap();
            let first = grass_session::create_session(
                cache,
                user.id,
                current.auth_version,
                Duration::from_secs(300),
            )
            .await?;
            let second = grass_session::create_session(
                cache,
                user.id,
                current.auth_version,
                Duration::from_secs(300),
            )
            .await?;
            for sid in [&first, &second] {
                ensure!(
                    session::validate_current_session(&state, sid, "test.active")
                        .await?
                        .is_some()
                );
            }
            // A refresh may read an old session before the password transaction commits.
            let key = format!("session:{second}");
            let stale_refresh = cache.get(&key).await?.unwrap();
            let (uri, body) = match flow {
                "change" => (
                    "/api/v1/me/password".into(),
                    serde_json::json!({
                        "current_password": current_password,
                        "password": next_password,
                    }),
                ),
                "reset" => {
                    let token = authentication::create_auth_token(
                        db,
                        user.id,
                        AuthTokenKind::PasswordReset,
                        time::Duration::hours(1),
                    )
                    .await?;
                    (
                        "/api/v1/auth/password/reset".into(),
                        serde_json::json!({
                            "token": token,
                            "password": next_password,
                        }),
                    )
                }
                _ => (
                    format!("/api/v1/admin/users/{}/reset-password", user.id),
                    serde_json::json!({ "password": next_password }),
                ),
            };
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .method("POST")
                        .header("content-type", "application/json")
                        .header("cookie", format!("session_id={first}"))
                        .body(Body::from(body.to_string()))?,
                )
                .await?;
            ensure!(
                response.status().is_success(),
                "password flow {flow} failed with {}",
                response.status()
            );
            let updated = users::get_user_by_id(db, user.id).await?.unwrap();
            ensure!(updated.auth_version == current.auth_version + 1);
            // Complete that delayed cache write after revocation; DB state must still win.
            ensure!(
                cache
                    .update_if_present(&key, &stale_refresh, Duration::from_secs(300))
                    .await?
            );
            for (sid, method, path) in [
                (&first, "GET", "/protected"),
                (&second, "POST", "/protected"),
                (&second, "GET", "/admin"),
            ] {
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .uri(path)
                            .method(method)
                            .header("cookie", format!("session_id={sid}"))
                            .body(Body::empty())?,
                    )
                    .await?;
                ensure!(
                    response.status().as_u16() == 401,
                    "old session survived {flow}"
                );
            }
            ensure!(
                users::verify_user_password(db, &user.email, next_password)
                    .await?
                    .is_some()
            );
            ensure!(
                users::verify_user_password(db, &user.email, current_password)
                    .await?
                    .is_none()
            );
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/auth/login")
                        .method("POST")
                        .extension(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                            [127, 0, 0, 1],
                            12345,
                        ))))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "email": user.email,
                                "password": next_password,
                            })
                            .to_string(),
                        ))?,
                )
                .await?;
            ensure!(
                response.status().is_success(),
                "new password could not log in after {flow}"
            );
            let sid = response
                .headers()
                .get_all("set-cookie")
                .iter()
                .filter_map(|cookie| cookie.to_str().ok())
                .find_map(|cookie| {
                    cookie
                        .strip_prefix("session_id=")
                        .and_then(|value| value.split(';').next())
                })
                .context("login did not issue a session")?;
            ensure!(
                session::validate_current_session(&state, sid, "test.login")
                    .await?
                    .is_some()
            );
            grass_session::revoke_session(cache, sid).await?;
            current_password = next_password;
        }
        let current = users::get_user_by_id(db, user.id).await?.unwrap();
        let first = grass_session::create_session(
            cache,
            user.id,
            current.auth_version,
            Duration::from_secs(300),
        )
        .await?;
        let second = grass_session::create_session(
            cache,
            user.id,
            current.auth_version,
            Duration::from_secs(300),
        )
        .await?;
        ensure!(
            session::validate_current_session(&state, &first, "test.before-disable")
                .await?
                .is_some()
        );
        let disabled = users::update_user(
            db,
            current.clone(),
            users::UpdateUserParams {
                display_name: None,
                status: Some(UserStatus::Disabled),
                platform_role: None,
            },
        )
        .await?;
        ensure!(disabled.auth_version == current.auth_version + 1);
        ensure!(
            session::validate_current_session(&state, &first, "test.disabled")
                .await?
                .is_none()
        );
        let enabled = users::update_user(
            db,
            disabled,
            users::UpdateUserParams {
                display_name: None,
                status: Some(UserStatus::Active),
                platform_role: None,
            },
        )
        .await?;
        ensure!(enabled.auth_version == current.auth_version + 1);
        ensure!(
            session::validate_current_session(&state, &second, "test.reenabled")
                .await?
                .is_none()
        );
        let fresh = grass_session::create_session(
            cache,
            user.id,
            enabled.auth_version,
            Duration::from_secs(300),
        )
        .await?;
        ensure!(
            session::validate_current_session(&state, &fresh, "test.enabled")
                .await?
                .is_some()
        );
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE users SET deleted_at = CURRENT_TIMESTAMP WHERE id = $1",
            [user.id.into()],
        ))
        .await?;
        ensure!(
            session::validate_current_session(&state, &fresh, "test.deleted")
                .await?
                .is_none()
        );
        Ok(())
    }
    .await;
    database.cleanup().await?;
    result
}
