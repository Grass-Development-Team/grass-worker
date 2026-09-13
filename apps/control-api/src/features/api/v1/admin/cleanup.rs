pub(crate) mod audit_events;
pub(crate) mod build_logs;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(audit_events::router())
        .merge(build_logs::router())
}
