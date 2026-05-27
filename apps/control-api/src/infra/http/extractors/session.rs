use axum::{extract::FromRequestParts, http::request::Parts};

use crate::infra::error::AppError;

pub struct Session {
    pub data: grass_session::SessionData,
    pub session_id: String,
}

pub struct OptionalSession(pub Option<Session>);

impl<S> FromRequestParts<S> for OptionalSession
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let session_entry = parts
            .extensions
            .get::<Option<(String, grass_session::SessionData)>>()
            .cloned()
            .flatten();

        Ok(OptionalSession(
            session_entry.map(|(session_id, data)| Session { data, session_id }),
        ))
    }
}

impl<S> FromRequestParts<S> for Session
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let OptionalSession(maybe) = OptionalSession::from_request_parts(parts, state)
            .await
            .unwrap();
        maybe.ok_or_else(|| AppError::Unauthorized {
            op: "auth.session_required",
            message: "authentication required".to_owned(),
        })
    }
}
