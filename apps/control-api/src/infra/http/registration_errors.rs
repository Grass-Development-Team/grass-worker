use crate::{domain::registration, infra::error::AppError};

pub(crate) fn map_registration_access_error(
    error: registration::RegistrationAccessError,
    op: &'static str,
) -> AppError {
    use crate::domain::codes::CodeUseError;
    use registration::RegistrationAccessError;

    match error {
        RegistrationAccessError::InvalidPolicy => AppError::Internal {
            op,
            message: error.to_string(),
        },
        RegistrationAccessError::Closed | RegistrationAccessError::CredentialRequired => {
            AppError::Forbidden {
                op,
                message: error.to_string(),
            }
        }
        RegistrationAccessError::Code(CodeUseError::NotFound | CodeUseError::WrongScope) => {
            AppError::Forbidden {
                op,
                message: "registration code is invalid".to_owned(),
            }
        }
        RegistrationAccessError::Code(CodeUseError::Used) => AppError::Conflict {
            op,
            message: error.to_string(),
        },
        RegistrationAccessError::Code(CodeUseError::Expired) => AppError::Gone {
            op,
            message: error.to_string(),
        },
        RegistrationAccessError::Code(CodeUseError::Revoked) => AppError::Forbidden {
            op,
            message: error.to_string(),
        },
        RegistrationAccessError::Code(CodeUseError::Database(source))
        | RegistrationAccessError::Database(source) => AppError::Infrastructure { op, source },
    }
}
