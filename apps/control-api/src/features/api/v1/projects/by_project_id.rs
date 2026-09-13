pub(crate) mod archive;
pub(crate) mod delete;
pub(crate) mod deployments;
pub(crate) mod hard_delete;
pub(crate) mod hosts;
pub(crate) mod restore;
pub(crate) mod serve_nodes;
pub(crate) mod source_credential;
pub(crate) mod transfer_team;
pub(crate) mod unarchive;

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::projects::{self, UpdateProjectParams},
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, project},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route(
            "/projects/{project_id}",
            axum::routing::get(get).patch(update),
        )
        .merge(archive::router())
        .merge(delete::router())
        .merge(deployments::router())
        .merge(hard_delete::router())
        .merge(hosts::router())
        .merge(restore::router())
        .merge(serve_nodes::router())
        .merge(source_credential::router())
        .merge(transfer_team::router())
        .merge(unarchive::router())
}

fn validate_repository_url(value: &str) -> Result<(), &'static str> {
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

fn project_view(project: &project::Model) -> ProjectResponse {
    ProjectResponse {
        id: project.id,
        team_id: project.team_id,
        slug: project.slug.clone(),
        name: project.name.clone(),
        runtime: projects::runtime_value(&project.runtime),
        repository_url: project.repository_url.clone(),
        default_branch: project.default_branch.clone(),
        install_command: project.install_command.clone(),
        build_command: project.build_command.clone(),
        output_directory: project.output_directory.clone(),
        source_config: project.source_config.clone(),
        build_config: project.build_config.clone(),
        archived_at: project.archived_at,
        deleted_at: project.deleted_at,
        created_at: project.created_at,
        updated_at: project.updated_at,
    }
}

fn optional_trimmed(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim().to_owned();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

/// GET /api/v1/projects/{project_id}
async fn get(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.detail";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;

    Ok(ok_response(GetResponse {
        project: project_view(&access.project),
        team: GetTeamResponse {
            id: access.team.id,
            slug: access.team.slug.clone(),
            name: access.team.name.clone(),
        },
        role: role_value(&access.role),
    }))
}

/// PATCH semantics: an absent field is unchanged; an empty string clears the
/// value; anything else replaces it.
#[derive(Deserialize)]
struct UpdateProjectRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    repository_url: Option<String>,
    #[serde(default)]
    default_branch: Option<String>,
    #[serde(default)]
    install_command: Option<String>,
    #[serde(default)]
    build_command: Option<String>,
    #[serde(default)]
    output_directory: Option<String>,
    #[serde(default)]
    root_directory: Option<String>,
    #[serde(default)]
    framework_hint: Option<String>,
}

fn optional_source_value(project: &project::Model, key: &str) -> Value {
    project
        .source_config
        .get(key)
        .cloned()
        .unwrap_or(Value::Null)
}

fn command_state(value: &Option<String>) -> Value {
    json!({ "configured": value.is_some() })
}

fn project_update_changes(before: &project::Model, after: &project::Model) -> (Value, Vec<String>) {
    let mut before_values = Map::new();
    let mut after_values = Map::new();
    let mut changed_fields = Vec::new();

    macro_rules! record_change {
        ($field:literal, $before:expr, $after:expr) => {
            if $before != $after {
                before_values.insert($field.to_owned(), json!($before));
                after_values.insert($field.to_owned(), json!($after));
                changed_fields.push($field.to_owned());
            }
        };
    }

    record_change!("name", &before.name, &after.name);
    record_change!(
        "repository_url",
        &before.repository_url,
        &after.repository_url
    );
    record_change!(
        "default_branch",
        &before.default_branch,
        &after.default_branch
    );
    record_change!(
        "output_directory",
        &before.output_directory,
        &after.output_directory
    );
    record_change!(
        "root_directory",
        optional_source_value(before, "root_directory"),
        optional_source_value(after, "root_directory")
    );
    record_change!(
        "framework_hint",
        optional_source_value(before, "framework_hint"),
        optional_source_value(after, "framework_hint")
    );

    for (field, before_command, after_command) in [
        (
            "install_command",
            &before.install_command,
            &after.install_command,
        ),
        ("build_command", &before.build_command, &after.build_command),
    ] {
        if before_command != after_command {
            before_values.insert(field.to_owned(), command_state(before_command));
            after_values.insert(field.to_owned(), command_state(after_command));
            changed_fields.push(field.to_owned());
        }
    }

    (
        json!({
            "before": before_values,
            "after": after_values,
        }),
        changed_fields,
    )
}

/// PATCH /api/v1/projects/{project_id}
async fn update(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
    Json(body): Json<UpdateProjectRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.update";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    access.require_member(OP)?;
    let db = crate::infra::http::database(&state, OP)?;

    if let Some(name) = &body.name
        && name.trim().is_empty()
    {
        return Err(AppError::Validation {
            op: OP,
            message: "name cannot be empty".to_owned(),
        });
    }

    let mut source_config = access.project.source_config.clone();
    let mut source_config_changed = false;
    if let Some(root_directory) = body.root_directory {
        source_config["root_directory"] = json!(optional_trimmed(Some(root_directory)));
        source_config_changed = true;
    }
    if let Some(framework_hint) = body.framework_hint {
        source_config["framework_hint"] = json!(optional_trimmed(Some(framework_hint)));
        source_config_changed = true;
    }

    if let Some(url) = &body.repository_url {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            validate_repository_url(trimmed).map_err(|message| AppError::Validation {
                op: OP,
                message: message.to_owned(),
            })?;
        }
    }

    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    if let Some(url) = &body.repository_url {
        let trimmed = url.trim();
        let bound =
            crate::domain::source_credentials::bound_credential(&transaction, access.project.id)
                .await
                .map_err(|error| AppError::Infrastructure {
                    op: OP,
                    source: anyhow::Error::new(error),
                })?;
        if bound.is_some_and(|credential| {
            trimmed.is_empty()
                || !crate::domain::source_credentials::matches_repository_url(&credential, trimmed)
        }) {
            return Err(AppError::Conflict {
                op: OP,
                message:
                    "unbind the source credential before changing repository scheme, host, or port"
                        .to_owned(),
            });
        }
    }

    let clear_or_set = |value: Option<String>| value.map(|value| optional_trimmed(Some(value)));

    let before = access.project.clone();
    let project = projects::update(
        &transaction,
        access.project,
        UpdateProjectParams {
            name: body.name.map(|name| name.trim().to_owned()),
            repository_url: clear_or_set(body.repository_url),
            default_branch: clear_or_set(body.default_branch),
            install_command: clear_or_set(body.install_command),
            build_command: clear_or_set(body.build_command),
            output_directory: clear_or_set(body.output_directory),
            source_config: source_config_changed.then_some(source_config),
            build_config: None,
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let (changes, changed_fields) = project_update_changes(&before, &project);
    audits::create_audit_event_with_changes(
        &transaction,
        CreateAuditEventParams {
            actor_user_id: Some(session.data.user_id),
            actor_node_id: None,
            team_id: Some(project.team_id),
            action: "project.updated".to_owned(),
            target_type: "project".to_owned(),
            target_id: Some(project.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "changed_fields": changed_fields }),
        },
        changes,
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(UpdateResponse {
        project: project_view(&project),
    }))
}

fn role_value(role: &crate::infra::database::entity::TeamMemberRole) -> &'static str {
    use crate::infra::database::entity::TeamMemberRole;

    match role {
        TeamMemberRole::Owner => "owner",
        TeamMemberRole::Admin => "admin",
        TeamMemberRole::Member => "member",
        TeamMemberRole::Viewer => "viewer",
    }
}

#[derive(serde::Serialize)]
struct ProjectResponse {
    id: uuid::Uuid,
    team_id: uuid::Uuid,
    slug: String,
    name: String,
    runtime: &'static str,
    repository_url: Option<String>,
    default_branch: Option<String>,
    install_command: Option<String>,
    build_command: Option<String>,
    output_directory: Option<String>,
    source_config: serde_json::Value,
    build_config: serde_json::Value,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    archived_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    deleted_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    updated_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct GetTeamResponse {
    id: uuid::Uuid,
    slug: String,
    name: String,
}

#[derive(serde::Serialize)]
struct GetResponse {
    project: ProjectResponse,
    team: GetTeamResponse,
    role: &'static str,
}

#[derive(serde::Serialize)]
struct UpdateResponse {
    project: ProjectResponse,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::database::entity::ProjectRuntime;
    use crate::infra::database::entity::project;
    use serde_json::json;
    use time::OffsetDateTime;
    use uuid::Uuid;
    fn project() -> project::Model {
        project::Model {
            id: Uuid::now_v7(),
            team_id: Uuid::now_v7(),
            created_by_user_id: Some(Uuid::now_v7()),
            slug: "demo".to_owned(),
            name: "Demo".to_owned(),
            runtime: ProjectRuntime::Static,
            repository_url: Some("https://example.com/team/demo.git".to_owned()),
            default_branch: Some("main".to_owned()),
            install_command: Some("install --token secret".to_owned()),
            build_command: Some("build --api-key secret".to_owned()),
            output_directory: Some("dist".to_owned()),
            source_config: json!({
                "root_directory": "apps/site",
                "framework_hint": "vite",
                "credential": "must-not-appear",
            }),
            build_config: json!({}),
            archived_at: None,
            deleted_at: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn project_update_audit_records_effective_changes_without_command_contents() {
        let before = project();
        let after = project::Model {
            name: "Renamed".to_owned(),
            install_command: Some("install --token another-secret".to_owned()),
            source_config: json!({
                "root_directory": "apps/web",
                "framework_hint": "vite",
                "credential": "different-secret",
            }),
            ..before.clone()
        };

        let (changes, fields) = project_update_changes(&before, &after);
        let encoded = changes.to_string();

        assert_eq!(fields, ["name", "root_directory", "install_command"]);
        assert_eq!(
            changes["before"]["install_command"],
            json!({ "configured": true })
        );
        assert_eq!(
            changes["after"]["install_command"],
            json!({ "configured": true })
        );
        assert!(!encoded.contains("another-secret"));
        assert!(!encoded.contains("must-not-appear"));
        assert!(!encoded.contains("different-secret"));
    }
}
