pub(crate) mod deployments;
pub(crate) mod log_stream;
pub(crate) mod nodes;
pub(crate) mod serve;
use axum::{Router, middleware};

use crate::{infra::http::middlewares::node_auth, state::ControlApiState};
pub(crate) fn router(state: ControlApiState) -> Router<ControlApiState> {
    Router::new()
        .merge(deployments::router())
        .merge(nodes::router())
        .merge(serve::router())
        .merge(log_stream::router())
        .layer(middleware::from_fn_with_state(
            state,
            node_auth::node_auth_middleware,
        ))
}
