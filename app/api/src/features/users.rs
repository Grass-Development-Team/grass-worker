use crate::AppState;
use axum::{Extension, Json, Router, http::StatusCode, response::IntoResponse, routing::get};
use axum_extra::extract::CookieJar;
use grass_worker_database::entities::user;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Serialize)]
struct UserResponse {
    id: Uuid,
    email: String,
    is_admin: bool,
    is_initial_admin: bool,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<user::Model> for UserResponse {
    fn from(value: user::Model) -> Self {
        Self {
            id: value.id,
            email: value.email,
            is_admin: value.is_admin,
            is_initial_admin: value.is_initial_admin,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct UsersEnvelope {
    users: Vec<UserResponse>,
}

pub fn install_user_routes(router: Router, state: AppState) -> Router {
    let user_router = Router::new()
        .route("/api/v1/admin/users", get(list_users))
        .layer(Extension(state));

    router.merge(user_router)
}

async fn list_users(
    Extension(state): Extension<AppState>,
    jar: CookieJar,
) -> axum::response::Response {
    let actor = match authenticated_user(&state, jar).await {
        Ok(actor) => actor,
        Err(response) => return response,
    };

    match state.users.list_all(state.database.as_ref(), &actor).await {
        Ok(users) => Json(UsersEnvelope {
            users: users.into_iter().map(UserResponse::from).collect(),
        })
        .into_response(),
        Err(error) => user_error_response(error),
    }
}

async fn authenticated_user(
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

fn user_error_response(error: crate::domain::user::UserError) -> axum::response::Response {
    match error.kind() {
        crate::domain::user::UserErrorKind::Forbidden => {
            error_response(StatusCode::FORBIDDEN, error.message())
        }
        crate::domain::user::UserErrorKind::Internal => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal user error")
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
