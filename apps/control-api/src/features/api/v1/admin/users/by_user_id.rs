pub(crate) mod mfa;
pub(crate) mod reset_password;

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::users::UpdateUserParams,
    infra::{
        database::entity::{PlatformRole, UserStatus, user},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/users/{user_id}", axum::routing::patch(update))
        .merge(mfa::router())
        .merge(reset_password::router())
}

fn user_view(user: &user::Model) -> UserResponse {
    UserResponse {
        id: user.id,
        email: user.email.clone(),
        display_name: user.display_name.clone(),
        status: user.status.as_str(),
        platform_role: user.platform_role.as_str(),
        email_verified: user.email_verified_at.is_some(),
        last_login_at: user.last_login_at,
        created_at: user.created_at,
    }
}

#[derive(Deserialize)]
struct UpdateUserRequest {
    /// Explicit `null` clears the display name.
    #[serde(default, deserialize_with = "crate::infra::http::patch::nullable")]
    display_name: Option<Option<String>>,
    status: Option<String>,
    platform_role: Option<String>,
}

/// PATCH /api/v1/admin/users/{user_id}
async fn update(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(user_id): Path<Uuid>,
    Json(body): Json<UpdateUserRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.users.update";
    let status = body
        .status
        .as_deref()
        .map(|value| {
            UserStatus::parse(value).ok_or_else(|| AppError::Validation {
                op: OP,
                message: format!("unknown user status: {value}"),
            })
        })
        .transpose()?;
    let platform_role = body
        .platform_role
        .as_deref()
        .map(|value| {
            PlatformRole::parse(value).ok_or_else(|| AppError::Validation {
                op: OP,
                message: format!("unknown platform role: {value}"),
            })
        })
        .transpose()?;
    let display_name = body.display_name.map(|name| {
        name.map(|value| value.trim().to_owned())
            .filter(|v| !v.is_empty())
    });
    if display_name
        .as_ref()
        .and_then(|name| name.as_ref())
        .is_some_and(|name| name.chars().count() > 120)
    {
        return Err(AppError::Validation {
            op: OP,
            message: "display name must not exceed 120 characters".to_owned(),
        });
    }

    let updated = crate::domain::admin_users::update(
        &state,
        data.user_id,
        user_id,
        UpdateUserParams {
            display_name,
            status,
            platform_role,
        },
    )
    .await?;
    Ok(ok_response(UpdateResponse {
        user: user_view(&updated),
    }))
}

#[derive(serde::Serialize)]
struct UserResponse {
    id: uuid::Uuid,
    email: String,
    display_name: Option<String>,
    status: &'static str,
    platform_role: &'static str,
    email_verified: bool,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    last_login_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct UpdateResponse {
    user: UserResponse,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_json_preserves_display_name_patch_intent() {
        for (raw, expected) in [
            ("{}", None),
            (r#"{"display_name":null}"#, Some(None)),
            (
                r#"{"display_name":"Admin"}"#,
                Some(Some("Admin".to_owned())),
            ),
        ] {
            let request: UpdateUserRequest = serde_json::from_str(raw).unwrap();
            assert_eq!(request.display_name, expected);
        }
        assert!(serde_json::from_str::<UpdateUserRequest>(r#"{"display_name":false}"#).is_err());
    }
}
