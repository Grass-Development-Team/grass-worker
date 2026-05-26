use axum::{
    body::Body,
    extract::FromRequestParts,
    http::{HeaderMap, Request, request::Parts},
    middleware::Next,
    response::Response,
};

use crate::state::ControlApiState;

pub struct Session {
    pub data: grass_session::SessionData,
    pub session_id: String,
}

pub struct OptionalSession(pub Option<Session>);

const SESSION_COOKIE: &str = "session_id";
const IDLE_TTL_SECONDS: u64 = 900;

pub async fn session_middleware(
    state: axum::extract::State<ControlApiState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let session_id = extract_session_cookie(request.headers());

    match (session_id, state.try_redis().cloned()) {
        (Some(sid), Some(conn)) => {
            let mut conn = conn;
            let session_data = grass_session::validate_session(&mut conn, &sid, IDLE_TTL_SECONDS)
                .await
                .ok()
                .flatten();
            request
                .extensions_mut()
                .insert::<Option<(String, grass_session::SessionData)>>(
                    session_data.map(|data| (sid, data)),
                );
        }
        _ => {
            request
                .extensions_mut()
                .insert::<Option<(String, grass_session::SessionData)>>(None);
        }
    }

    next.run(request).await
}

fn extract_session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookie_str| {
            cookie_str.split(';').find_map(|pair| {
                let (name, value) = pair.trim().split_once('=')?;
                if name == SESSION_COOKIE {
                    Some(value.to_owned())
                } else {
                    None
                }
            })
        })
}

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
    type Rejection = crate::infra::error::AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let OptionalSession(maybe) = OptionalSession::from_request_parts(parts, state)
            .await
            .unwrap();
        maybe.ok_or_else(|| crate::infra::error::AppError::Unauthorized {
            op: "auth.session_required",
            message: "authentication required".to_owned(),
        })
    }
}
