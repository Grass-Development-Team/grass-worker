use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{infra::error::AppError, init, state::ControlApiState};

async fn setup_finished(state: &ControlApiState) -> Result<bool, AppError> {
    if let Some(db) = state.try_database() {
        return init::is_setup_finished(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: "setup.mode.read_state",
                source,
            });
    }

    if state.config.read().unwrap().database.url.trim().is_empty() {
        Ok(false)
    } else {
        Err(AppError::Internal {
            op: "setup.mode.database_unavailable",
            message: "database service unavailable".to_owned(),
        })
    }
}

pub async fn require_setup_mode(
    State(state): State<ControlApiState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let finished = match setup_finished(&state).await {
        Ok(finished) => finished,
        Err(error) => return error.into_response(),
    };
    if finished {
        return StatusCode::NOT_FOUND.into_response();
    }
    next.run(request).await
}

pub async fn require_ready_mode(
    State(state): State<ControlApiState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let finished = match setup_finished(&state).await {
        Ok(finished) => finished,
        Err(error) => return error.into_response(),
    };
    if !finished {
        return StatusCode::NOT_FOUND.into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use crate::infra::config::ControlApiConfig;

    use super::*;

    #[tokio::test]
    async fn missing_database_is_setup_only_when_unconfigured() {
        let setup_state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        assert!(!setup_finished(&setup_state).await.unwrap());

        let mut configured = ControlApiConfig::default();
        configured.database.url = "postgres://configured.example/grass".to_owned();
        let unavailable_state = ControlApiState::new(configured, "unused.toml");
        assert!(setup_finished(&unavailable_state).await.is_err());
    }
}
