pub(crate) mod announcements;
pub(crate) mod audit_events;
pub(crate) mod cleanup;
pub(crate) mod codes;
pub(crate) mod deployments;
pub(crate) mod domain_https;
pub(crate) mod domains;
pub(crate) mod host_sources;
pub(crate) mod identity_providers;
pub(crate) mod nodes;
pub(crate) mod projects;
pub(crate) mod quota_plans;
pub(crate) mod regional_ingresses;
pub(crate) mod regions;
pub(crate) mod registration;
pub(crate) mod reviews;
pub(crate) mod settings;
pub(crate) mod status;
pub(crate) mod storage;
pub(crate) mod team_groups;
pub(crate) mod teams;
pub(crate) mod users;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(announcements::router())
        .merge(audit_events::router())
        .merge(cleanup::router())
        .merge(codes::router())
        .merge(deployments::router())
        .merge(domain_https::router())
        .merge(domains::router())
        .merge(host_sources::router())
        .merge(identity_providers::router())
        .merge(nodes::router())
        .merge(projects::router())
        .merge(quota_plans::router())
        .merge(regional_ingresses::router())
        .merge(regions::router())
        .merge(registration::router())
        .merge(reviews::router())
        .merge(settings::router())
        .merge(status::router())
        .merge(storage::router())
        .merge(team_groups::router())
        .merge(teams::router())
        .merge(users::router())
}
