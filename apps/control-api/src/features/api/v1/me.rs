use crate::{
    domain::users,
    infra::{
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};
use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;

pub async fn handler(
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

    Ok(ok_response(serde_json::json!({
        "user": super::auth::user_data(&user),
    })))
}

#[derive(Default, Deserialize)]
pub struct UpdateMeRequest {
    pub display_name: Option<Option<String>>,
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

pub async fn update(
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

    Ok(ok_response(serde_json::json!({
        "user": super::auth::user_data(&user),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
