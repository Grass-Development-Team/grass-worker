pub mod csrf_token;
pub mod login;
pub mod logout;

use axum::{
    Router,
    routing::{get, post},
};

use crate::state::ControlApiState;

pub fn router() -> Router<ControlApiState> {
    Router::new()
        .route("/login", post(login::handler))
        .route("/logout", post(logout::handler))
        .route("/csrf", get(csrf_token::handler))
}
