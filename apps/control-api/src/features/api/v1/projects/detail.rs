use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::TransactionTrait;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        projects::{self, UpdateProjectParams},
    },
    infra::{
        database::entity::{AuditEventResult, project},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

/// GET /api/v1/projects/{project_id}
pub async fn get(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.detail";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;

    Ok(ok_response(json!({
        "project": super::project_view(&access.project),
        "team": {
            "id": access.team.id,
            "slug": access.team.slug,
            "name": access.team.name,
        },
        "role": crate::features::api::v1::teams::role_value(&access.role),
    })))
}

/// PATCH semantics: an absent field is unchanged; an empty string clears the
/// value; anything else replaces it.
#[derive(Deserialize)]
pub struct UpdateProjectRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub repository_url: Option<String>,
    #[serde(default)]
    pub default_branch: Option<String>,
    #[serde(default)]
    pub install_command: Option<String>,
    #[serde(default)]
    pub build_command: Option<String>,
    #[serde(default)]
    pub output_directory: Option<String>,
    #[serde(default)]
    pub root_directory: Option<String>,
    #[serde(default)]
    pub framework_hint: Option<String>,
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
pub async fn update(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
    Json(body): Json<UpdateProjectRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.update";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_member(OP)?;
    let db = super::database(&state, OP)?;

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
        source_config["root_directory"] = json!(super::optional_trimmed(Some(root_directory)));
        source_config_changed = true;
    }
    if let Some(framework_hint) = body.framework_hint {
        source_config["framework_hint"] = json!(super::optional_trimmed(Some(framework_hint)));
        source_config_changed = true;
    }

    if let Some(url) = &body.repository_url {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            super::validate_repository_url(trimmed).map_err(|message| AppError::Validation {
                op: OP,
                message: message.to_owned(),
            })?;
        }
    }

    let transaction = db
        .begin()
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

    let clear_or_set =
        |value: Option<String>| value.map(|value| super::optional_trimmed(Some(value)));

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

    Ok(ok_response(json!({
        "project": super::project_view(&project),
    })))
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use crate::infra::database::entity::ProjectRuntime;

    use super::*;

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
