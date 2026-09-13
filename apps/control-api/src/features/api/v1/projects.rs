use crate::infra::http::{cache, database};

pub mod create;

pub(crate) mod by_project_id;

pub mod detail;

pub mod host_certificates;

pub mod hosts;

pub mod lifecycle;

pub mod list;

pub mod source_credentials;

use axum::{
    Router,
    routing::{get, post},
};

use crate::{
    domain::projects,
    infra::{database::entity::project, http::timestamps::ts},
    state::ControlApiState,
};

pub fn router() -> Router<ControlApiState> {
    Router::new()
        .route("/projects", get(list::handler).post(create::handler))
        .route(
            "/projects/{project_id}",
            get(detail::get).patch(detail::update),
        )
        .route(
            "/projects/{project_id}/source-credential",
            get(source_credentials::get)
                .post(source_credentials::bind)
                .delete(source_credentials::unbind),
        )
        .route("/projects/{project_id}/archive", post(lifecycle::archive))
        .route(
            "/projects/{project_id}/unarchive",
            post(lifecycle::unarchive),
        )
        .route("/projects/{project_id}/delete", post(lifecycle::delete))
        .route("/projects/{project_id}/restore", post(lifecycle::restore))
        .route(
            "/projects/{project_id}/transfer-team",
            post(lifecycle::transfer_team),
        )
        .route(
            "/projects/{project_id}/hard-delete",
            post(lifecycle::hard_delete),
        )
        .route(
            "/projects/{project_id}/hosts",
            get(hosts::list).post(hosts::create),
        )
        .route(
            "/projects/{project_id}/hosts/{host_id}",
            axum::routing::patch(hosts::update).delete(hosts::remove),
        )
        .route(
            "/projects/{project_id}/hosts/{host_id}/primary",
            post(hosts::set_primary),
        )
        .route(
            "/projects/{project_id}/hosts/{host_id}/verify",
            post(hosts::verify),
        )
        .route(
            "/projects/{project_id}/hosts/{host_id}/certificate",
            get(host_certificates::get).patch(host_certificates::update),
        )
        .route(
            "/projects/{project_id}/hosts/{host_id}/certificate/renew",
            post(host_certificates::renew),
        )
        .route(
            "/projects/{project_id}/hosts/{host_id}/certificate/import",
            post(host_certificates::import),
        )
        .route(
            "/projects/{project_id}/hosts/{host_id}/provision",
            post(hosts::provision),
        )
        .merge(by_project_id::router())
}

pub(crate) fn validate_repository_url(value: &str) -> Result<(), &'static str> {
    grass_git_source::parse_repository_url(value)
        .map(|_| ())
        .map_err(|error| match error {
            grass_git_source::RepositoryUrlError::UnsupportedTransport => {
                "repository_url transport is not supported"
            }
            grass_git_source::RepositoryUrlError::EmbeddedCredential => {
                "repository_url must not contain credentials"
            }
            grass_git_source::RepositoryUrlError::Invalid => "repository_url is invalid",
        })
}

pub(crate) fn project_view(project: &project::Model) -> serde_json::Value {
    serde_json::json!({
        "id": project.id,
        "team_id": project.team_id,
        "slug": project.slug,
        "name": project.name,
        "runtime": projects::runtime_value(&project.runtime),
        "repository_url": project.repository_url,
        "default_branch": project.default_branch,
        "install_command": project.install_command,
        "build_command": project.build_command,
        "output_directory": project.output_directory,
        "source_config": project.source_config,
        "build_config": project.build_config,
        "archived_at": ts(project.archived_at),
        "deleted_at": ts(project.deleted_at),
        "created_at": ts(project.created_at),
        "updated_at": ts(project.updated_at),
    })
}

pub(crate) fn optional_trimmed(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim().to_owned();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

#[cfg(test)]
mod repository_url_tests {
    use super::validate_repository_url;

    #[test]
    fn project_urls_support_all_approved_git_transports() {
        for value in [
            "http://example.com/repo.git",
            "https://example.com:8443/repo.git",
            "ssh://git@example.com:2222/repo.git",
            "git@example.com:repo.git",
            "git://example.com:19418/repo.git",
        ] {
            assert!(validate_repository_url(value).is_ok(), "rejected {value}");
        }
    }

    #[test]
    fn project_urls_return_stable_safe_validation_messages() {
        assert_eq!(
            validate_repository_url("file:///srv/repo"),
            Err("repository_url transport is not supported")
        );
        assert_eq!(
            validate_repository_url("https://token@example.com/repo.git"),
            Err("repository_url must not contain credentials")
        );
        assert_eq!(
            validate_repository_url("../repo"),
            Err("repository_url is invalid")
        );
    }
}
