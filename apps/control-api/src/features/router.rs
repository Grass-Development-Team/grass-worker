use axum::{Router, middleware};
use tracing::info;

use crate::infra::http::middlewares::session as session_mw;
use crate::{
    features::{api, frontend},
    state::ControlApiState,
};

pub fn router(state: ControlApiState) -> Router<ControlApiState> {
    info!(operation = "control_api.start", "Control API starting");

    let session_layer =
        middleware::from_fn_with_state(state.clone(), session_mw::session_middleware);

    Router::new()
        .merge(crate::features::health::router())
        .nest("/api", api::router(state.clone()).layer(session_layer))
        .fallback(frontend::frontend_fallback)
}
