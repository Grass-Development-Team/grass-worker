use crate::AppState;
use axum::{
    Extension, Json, Router,
    extract::rejection::JsonRejection,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Serialize)]
struct UserEnvelope {
    user: crate::domain::auth::AuthenticatedUser,
}

pub fn install_auth_routes(router: Router, state: AppState) -> Router {
    let auth_router = Router::new()
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/me", get(current_user))
        .layer(Extension(state));

    router.merge(auth_router)
}

async fn login(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
    payload: Result<Json<LoginRequest>, JsonRejection>,
) -> axum::response::Response {
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(error = %error, "login request rejected: invalid payload");
            return error_response(StatusCode::BAD_REQUEST, error.to_string());
        }
    };

    let input = crate::domain::auth::LoginInput {
        email: payload.email,
        password: payload.password,
    };

    match state.auth.login(state.database.as_ref(), input).await {
        Ok(session) => (
            state.auth.write_login_cookie(jar, &session.token),
            Json(UserEnvelope { user: session.user }),
        )
            .into_response(),
        Err(error) => auth_error_response(error),
    }
}

async fn current_user(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
) -> axum::response::Response {
    let Some(session_cookie) = jar.get(crate::domain::auth::SESSION_COOKIE_NAME) else {
        return auth_error_response(crate::domain::auth::AuthError::unauthorized(
            "missing session",
        ));
    };

    match state
        .auth
        .current_user(state.database.as_ref(), session_cookie.value())
        .await
    {
        Ok(user) => Json(UserEnvelope { user }).into_response(),
        Err(error) => auth_error_response(error),
    }
}

async fn logout(Extension(state): Extension<AppState>, jar: CookieJar) -> axum::response::Response {
    let session_token = jar
        .get(crate::domain::auth::SESSION_COOKIE_NAME)
        .map(|cookie| cookie.value().to_owned());
    if let Err(error) = state
        .auth
        .logout(state.database.as_ref(), session_token.as_deref())
        .await
    {
        tracing::warn!(
            error = error.message(),
            "logout completed with revoke failure"
        );
    }

    (
        crate::adapters::auth::clear_session_cookie(jar),
        StatusCode::NO_CONTENT,
    )
        .into_response()
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

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::{
        Argon2,
        password_hash::{PasswordHasher, SaltString},
    };
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
        routing::get,
    };
    use chrono::Utc;
    use grass_worker_database::entities::{user, user_password_credential, user_session};
    use rand_core::OsRng;
    use sea_orm::{
        DatabaseBackend, DatabaseConnection, DbErr, MockDatabase, MockDatabaseConnection,
        MockExecResult,
    };
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;
    use tracing::subscriber::set_default;
    use tracing_subscriber::{fmt, fmt::MakeWriter};
    use uuid::Uuid;

    #[derive(Clone, Default)]
    struct SharedLogBuffer(Arc<Mutex<Vec<u8>>>);

    struct SharedLogWriter(Arc<Mutex<Vec<u8>>>);

    impl SharedLogBuffer {
        fn contents(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl<'a> MakeWriter<'a> for SharedLogBuffer {
        type Writer = SharedLogWriter;

        fn make_writer(&'a self) -> Self::Writer {
            SharedLogWriter(self.0.clone())
        }
    }

    impl Write for SharedLogWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn wait_for_logs(logs: &SharedLogBuffer, needle: &str) -> String {
        for _ in 0..50 {
            let output = logs.contents();
            if output.contains(needle) {
                return output;
            }

            std::thread::yield_now();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        logs.contents()
    }

    fn hash_password_for_test(password: &str) -> String {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn login_sets_cookie_and_returns_user_payload() {
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-17T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let user = user::Model {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            email: "admin@example.com".to_owned(),
            is_admin: true,
            is_initial_admin: true,
            created_at,
            updated_at: created_at,
        };
        let credential = user_password_credential::Model {
            user_id: user.id,
            password_hash: hash_password_for_test("secret-pass"),
            password_updated_at: created_at,
        };
        let connection = Arc::new(MockDatabaseConnection::new(
            MockDatabase::new(DatabaseBackend::Postgres)
                .append_query_results([[user.clone()]])
                .append_query_results([[credential]])
                .append_exec_results([MockExecResult {
                    last_insert_id: 0,
                    rows_affected: 1,
                }]),
        ));
        let app = install_auth_routes(
            Router::new(),
            AppState::new(DatabaseConnection::MockDatabaseConnection(
                connection.clone(),
            )),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"email":" ADMIN@example.com ","password":"secret-pass"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let set_cookie = response
            .headers()
            .get(axum::http::header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        assert!(set_cookie.contains("gw_session="));
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Lax"));

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["user"]["id"], user.id.to_string());
        assert_eq!(payload["user"]["email"], "admin@example.com");
        assert_eq!(payload["user"]["is_admin"], true);
        assert_eq!(payload["user"]["is_initial_admin"], true);

        let transaction_log =
            DatabaseConnection::MockDatabaseConnection(connection).into_transaction_log();
        let statements = transaction_log
            .iter()
            .flat_map(|entry| entry.statements().iter())
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>();
        assert!(
            statements
                .iter()
                .any(|statement| statement.contains("'admin@example.com'"))
        );
        assert!(
            !statements
                .iter()
                .any(|statement| statement.contains(" ADMIN@example.com "))
        );
    }

    #[tokio::test]
    async fn current_user_requires_valid_session_cookie() {
        let app = install_auth_routes(
            Router::new(),
            AppState::new(MockDatabase::new(DatabaseBackend::Postgres).into_connection()),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["error"], "missing session");
    }

    #[tokio::test]
    async fn current_user_returns_user_envelope_for_valid_session_cookie() {
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-17T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let expires_at = chrono::DateTime::parse_from_rfc3339("2026-04-24T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let user = user::Model {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            email: "admin@example.com".to_owned(),
            is_admin: true,
            is_initial_admin: true,
            created_at,
            updated_at: created_at,
        };
        let session = grass_worker_database::entities::user_session::Model {
            id: Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            user_id: user.id,
            token_hash: crate::adapters::auth::hash_session_token("raw-session-token"),
            created_at,
            expires_at,
            revoked_at: None,
        };
        let app = install_auth_routes(
            Router::new(),
            AppState::new(
                MockDatabase::new(DatabaseBackend::Postgres)
                    .append_query_results([[session]])
                    .append_query_results([[user.clone()]])
                    .into_connection(),
            ),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/me")
                    .header(header::COOKIE, "gw_session=raw-session-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["user"]["id"], user.id.to_string());
        assert_eq!(payload["user"]["email"], "admin@example.com");
        assert_eq!(payload["user"]["is_admin"], true);
        assert_eq!(payload["user"]["is_initial_admin"], true);
    }

    #[tokio::test]
    async fn current_user_hashes_cookie_token_before_session_lookup() {
        let raw_token = "raw-session-token";
        let hashed_token = crate::adapters::auth::hash_session_token(raw_token);
        let connection = Arc::new(MockDatabaseConnection::new(
            MockDatabase::new(DatabaseBackend::Postgres)
                .append_query_results([Vec::<user_session::Model>::new()]),
        ));
        let app = install_auth_routes(
            Router::new(),
            AppState::new(DatabaseConnection::MockDatabaseConnection(
                connection.clone(),
            )),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/me")
                    .header(header::COOKIE, format!("gw_session={raw_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let transaction_log =
            DatabaseConnection::MockDatabaseConnection(connection).into_transaction_log();
        let statements = transaction_log
            .iter()
            .flat_map(|entry| entry.statements().iter())
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>();
        assert!(
            statements
                .iter()
                .any(|statement| statement.contains(&hashed_token))
        );
        assert!(
            !statements
                .iter()
                .any(|statement| statement.contains(raw_token))
        );
    }

    #[tokio::test]
    async fn current_user_returns_unauthorized_for_expired_session() {
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-17T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let expired_at = chrono::DateTime::parse_from_rfc3339("2026-04-18T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let session = user_session::Model {
            id: Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            user_id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            token_hash: crate::adapters::auth::hash_session_token("raw-session-token"),
            created_at,
            expires_at: expired_at,
            revoked_at: None,
        };
        let app = install_auth_routes(
            Router::new(),
            AppState::new(
                MockDatabase::new(DatabaseBackend::Postgres)
                    .append_query_results([[session]])
                    .into_connection(),
            ),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/me")
                    .header(header::COOKIE, "gw_session=raw-session-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["error"], "not authenticated");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn login_internal_failures_return_generic_error_message() {
        let logs = SharedLogBuffer::default();
        let subscriber = fmt()
            .with_writer(logs.clone())
            .with_ansi(false)
            .without_time()
            .finish();
        let _guard = set_default(subscriber);
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-17T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let user = user::Model {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            email: "admin@example.com".to_owned(),
            is_admin: true,
            is_initial_admin: true,
            created_at,
            updated_at: created_at,
        };
        let credential = user_password_credential::Model {
            user_id: user.id,
            password_hash: "not-a-valid-password-hash".to_owned(),
            password_updated_at: created_at,
        };
        let app = install_auth_routes(
            Router::new(),
            AppState::new(
                MockDatabase::new(DatabaseBackend::Postgres)
                    .append_query_results([[user]])
                    .append_query_results([[credential]])
                    .into_connection(),
            ),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"email":"admin@example.com","password":"secret-pass"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["error"], "internal auth error");
        assert!(!payload["error"].as_str().unwrap().contains("password"));
        assert!(!payload["error"].as_str().unwrap().contains("hash"));

        let output = wait_for_logs(&logs, "stored password hash is invalid");
        assert!(output.contains("stored password hash is invalid"));
        assert!(output.contains("error=password hash string missing field"));
    }

    #[tokio::test]
    async fn current_user_internal_failures_return_generic_error_message() {
        let app = install_auth_routes(
            Router::new(),
            AppState::new(
                MockDatabase::new(DatabaseBackend::Postgres)
                    .append_query_errors([DbErr::Custom("mock-db-error-details".to_owned())])
                    .into_connection(),
            ),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/me")
                    .header(header::COOKIE, "gw_session=raw-session-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["error"], "internal auth error");
        assert!(
            !payload["error"]
                .as_str()
                .unwrap()
                .contains("mock-db-error-details")
        );
    }

    #[tokio::test]
    async fn login_treats_whitespace_only_password_as_invalid_credentials() {
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-17T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let user = user::Model {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            email: "admin@example.com".to_owned(),
            is_admin: true,
            is_initial_admin: true,
            created_at,
            updated_at: created_at,
        };
        let credential = user_password_credential::Model {
            user_id: user.id,
            password_hash: hash_password_for_test("secret-pass"),
            password_updated_at: created_at,
        };
        let app = install_auth_routes(
            Router::new(),
            AppState::new(
                MockDatabase::new(DatabaseBackend::Postgres)
                    .append_query_results([[user]])
                    .append_query_results([[credential]])
                    .into_connection(),
            ),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"email":"admin@example.com","password":"   "}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["error"], "invalid credentials");
    }

    #[tokio::test]
    async fn logout_is_idempotent_and_clears_cookie() {
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
        assert!(set_cookie.contains("Max-Age=0"));
        assert!(set_cookie.contains("SameSite=Lax"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn logout_always_returns_204_even_when_revoke_fails() {
        let logs = SharedLogBuffer::default();
        let subscriber = fmt()
            .with_writer(logs.clone())
            .with_ansi(false)
            .without_time()
            .finish();
        let _guard = set_default(subscriber);
        let app = install_auth_routes(
            Router::new(),
            AppState::new(
                MockDatabase::new(DatabaseBackend::Postgres)
                    .append_exec_errors([DbErr::Custom("mock-revoke-error".to_owned())])
                    .into_connection(),
            ),
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/logout")
                    .header(header::COOKIE, "gw_session=stale-token")
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
        assert!(set_cookie.contains("Max-Age=0"));

        let output = wait_for_logs(&logs, "logout completed with revoke failure");
        assert!(output.contains("logout completed with revoke failure"));
        assert!(output.contains("mock-revoke-error"));
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
