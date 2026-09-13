pub(crate) mod admin;
pub(crate) mod database;
pub(crate) mod finish;
pub(crate) mod node;
pub(crate) mod site;
pub(crate) mod state;
pub(crate) mod storage;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(admin::router())
        .merge(database::router())
        .merge(finish::router())
        .merge(node::router())
        .merge(site::router())
        .merge(state::router())
        .merge(storage::router())
}
