pub(crate) mod artifact;
pub(crate) mod build_log;
pub(crate) mod source_credential;
pub(crate) mod ssh_host_key;
pub(crate) mod stage;
pub(crate) mod static_site;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(artifact::router())
        .merge(build_log::router())
        .merge(source_credential::router())
        .merge(ssh_host_key::router())
        .merge(stage::router())
        .merge(static_site::router())
}
