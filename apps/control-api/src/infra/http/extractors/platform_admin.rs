use axum::{extract::FromRequestParts, http::request::Parts};

use crate::{
    domain::users,
    infra::{database::entity::PlatformRole, error::AppError},
    state::ControlApiState,
};

use super::Session;

pub struct PlatformAdmin;

pub(crate) fn require_platform_admin_role(
    role: &PlatformRole,
    op: &'static str,
) -> Result<(), AppError> {
    if role != &PlatformRole::Admin {
        return Err(AppError::Forbidden {
            op,
            message: "platform administrator role required".to_owned(),
        });
    }
    Ok(())
}

impl FromRequestParts<ControlApiState> for PlatformAdmin {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ControlApiState,
    ) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state).await?;
        let db = state.try_database().ok_or_else(|| AppError::Internal {
            op: "platform_admin.no_database",
            message: "database not available".to_owned(),
        })?;
        let user = users::get_user_by_id(db, session.data.user_id)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: "platform_admin.lookup",
                source,
            })?
            .ok_or_else(|| AppError::Unauthorized {
                op: "platform_admin.user_not_found",
                message: "authenticated user not found".to_owned(),
            })?;

        require_platform_admin_role(&user.platform_role, "platform_admin.role_required")?;

        Ok(Self)
    }
}

#[cfg(test)]
mod tests {
    use crate::infra::database::entity::PlatformRole;

    use super::require_platform_admin_role;

    #[test]
    fn only_platform_administrators_are_authorized() {
        assert!(require_platform_admin_role(&PlatformRole::Admin, "test.admin").is_ok());
        assert!(require_platform_admin_role(&PlatformRole::User, "test.admin").is_err());
    }
}
