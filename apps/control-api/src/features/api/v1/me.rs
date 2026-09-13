pub(crate) mod avatar;
pub(crate) mod mfa;
pub(crate) mod password;
pub(crate) mod security;

use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::users,
    infra::{
        database::entity::user,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/me", axum::routing::get(handler).patch(update))
        .merge(avatar::router())
        .merge(mfa::router())
        .merge(password::router())
        .merge(security::router())
}

fn user_data(user: &user::Model) -> UserResponse {
    UserResponse {
        id: user.id,
        email: user.email.clone(),
        display_name: user.display_name.clone(),
        avatar_url: user_avatar_url(user.id, user.avatar_version),
        platform_role: user.platform_role.as_str(),
        email_verified: user.email_verified_at.is_some(),
    }
}

fn user_avatar_url(user_id: Uuid, version: Option<Uuid>) -> Option<String> {
    version.map(|version| format!("/api/v1/avatars/users/{user_id}/{version}/avatar.webp"))
}

async fn handler(
    State(state): State<ControlApiState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: "me.no_database",
        message: "database not available".to_owned(),
    })?;

    let user = users::get_user_by_id(db, session.data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "me.get_user",
            source,
        })?
        .ok_or_else(|| AppError::NotFound {
            op: "me.user_not_found",
            message: "user not found".to_owned(),
        })?;

    Ok(ok_response(ResponseBody {
        user: user_data(&user),
    }))
}

#[derive(Default, Deserialize)]
struct UpdateMeRequest {
    #[serde(default, deserialize_with = "crate::infra::http::patch::nullable")]
    display_name: Option<Option<String>>,
}

fn prepare_display_name(
    value: Option<Option<String>>,
    op: &'static str,
) -> Result<Option<Option<String>>, AppError> {
    let display_name = value.map(|value| {
        value
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
    });
    if display_name
        .as_ref()
        .and_then(|value| value.as_ref())
        .is_some_and(|value| value.chars().count() > 120)
    {
        return Err(AppError::Validation {
            op,
            message: "display name must not exceed 120 characters".to_owned(),
        });
    }
    if display_name.is_none() {
        return Err(AppError::Validation {
            op,
            message: "nothing to update".to_owned(),
        });
    }
    Ok(display_name)
}

async fn update(
    State(state): State<ControlApiState>,
    session: Session,
    Json(body): Json<UpdateMeRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "me.update";
    let display_name = prepare_display_name(body.display_name, OP)?;

    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let user = users::get_user_by_id(db, session.data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "user not found".to_owned(),
        })?;
    let user = users::update_user(
        db,
        user,
        users::UpdateUserParams {
            display_name,
            status: None,
            platform_role: None,
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(UpdateResponse {
        user: user_data(&user),
    }))
}

#[derive(serde::Serialize)]
struct UserResponse {
    id: uuid::Uuid,
    email: String,
    display_name: Option<String>,
    avatar_url: Option<String>,
    platform_role: &'static str,
    email_verified: bool,
}

#[derive(serde::Serialize)]
struct ResponseBody {
    user: UserResponse,
}

#[derive(serde::Serialize)]
struct UpdateResponse {
    user: UserResponse,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::database::entity::PlatformRole;
    use crate::infra::database::entity::UserStatus;
    use crate::infra::database::entity::user;
    use time::OffsetDateTime;
    use uuid::Uuid;

    #[test]
    fn authenticated_user_data_exposes_the_platform_role() {
        let now = OffsetDateTime::UNIX_EPOCH;
        let avatar_version = Uuid::max();
        let user = user::Model {
            auth_version: 1,
            id: Uuid::nil(),
            email: "admin@example.com".to_owned(),
            display_name: Some("Admin".to_owned()),
            avatar_version: Some(avatar_version),
            status: UserStatus::Active,
            platform_role: PlatformRole::Admin,
            email_verified_at: Some(now),
            last_login_at: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };

        assert_eq!(
            serde_json::to_value(user_data(&user)).unwrap()["platform_role"],
            "admin"
        );
        assert_eq!(
            serde_json::to_value(user_data(&user)).unwrap()["avatar_url"],
            format!(
                "/api/v1/avatars/users/{}/{avatar_version}/avatar.webp",
                Uuid::nil()
            )
        );
    }

    #[test]
    fn display_name_updates_are_normalized_and_bounded() {
        assert_eq!(
            prepare_display_name(Some(Some("  User Name  ".to_owned())), "test.me").unwrap(),
            Some(Some("User Name".to_owned()))
        );
        assert_eq!(
            prepare_display_name(Some(Some("   ".to_owned())), "test.me").unwrap(),
            Some(None)
        );
        assert_eq!(
            prepare_display_name(Some(None), "test.me").unwrap(),
            Some(None)
        );
        assert!(prepare_display_name(None, "test.me").is_err());
        assert!(prepare_display_name(Some(Some("x".repeat(121))), "test.me").is_err());
    }

    #[test]
    fn raw_json_distinguishes_missing_null_and_display_name_values() {
        let missing: UpdateMeRequest = serde_json::from_str("{}").unwrap();
        assert!(prepare_display_name(missing.display_name, "test").is_err());
        for (raw, expected) in [
            (r#"{"display_name":null}"#, None),
            (r#"{"display_name":"  "}"#, None),
            (r#"{"display_name":"  用户  "}"#, Some("用户".to_owned())),
        ] {
            let request: UpdateMeRequest = serde_json::from_str(raw).unwrap();
            assert_eq!(
                prepare_display_name(request.display_name, "test").unwrap(),
                Some(expected)
            );
        }
        assert!(serde_json::from_str::<UpdateMeRequest>(r#"{"display_name":42}"#).is_err());
    }

    #[tokio::test]
    async fn patch_null_clears_the_persisted_display_name_and_response() {
        use tower::ServiceExt;
        let before = crate::test_support::users::active_user();
        let mut after = before.clone();
        after.display_name = None;
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([vec![before.clone()], vec![after]])
            .into_connection();
        let log = db.clone();
        let state = ControlApiState::new(
            crate::infra::config::ControlApiConfig::default(),
            "unused.toml",
        );
        state.database.set(db).ok().unwrap();
        let now = time::OffsetDateTime::now_utc();
        let response = router()
            .with_state(state)
            .oneshot(
                axum::http::Request::builder()
                    .method("PATCH")
                    .uri("/me")
                    .header("content-type", "application/json")
                    .extension(Some((
                        "session".to_owned(),
                        grass_session::SessionData {
                            user_id: before.id,
                            auth_version: before.auth_version,
                            created_at: now,
                            last_accessed_at: now,
                        },
                    )))
                    .body(axum::body::Body::from(r#"{"display_name":null}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 8192)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(body["data"]["user"]["display_name"].is_null());
        let sql = format!("{:?}", log.into_transaction_log());
        assert!(
            sql.contains("UPDATE") && sql.contains("display_name") && sql.contains("NULL"),
            "{sql}"
        );
    }
}
