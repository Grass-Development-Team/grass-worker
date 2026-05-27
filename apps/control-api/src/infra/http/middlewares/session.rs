use axum::{
    body::Body,
    http::{HeaderMap, Request},
    middleware::Next,
    response::Response,
};

use crate::state::ControlApiState;

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
