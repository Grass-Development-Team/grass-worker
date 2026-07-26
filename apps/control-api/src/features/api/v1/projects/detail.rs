use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::projects::{self, UpdateProjectParams},
    infra::{
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

    if let Some(url) = &body.repository_url
        && !url.trim().is_empty()
        && url::Url::parse(url.trim()).is_err()
    {
        return Err(AppError::Validation {
            op: OP,
            message: "repository_url must be a valid URL".to_owned(),
        });
    }

    let clear_or_set =
        |value: Option<String>| value.map(|value| super::optional_trimmed(Some(value)));

    let project = projects::update(
        db,
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

    Ok(ok_response(json!({
        "project": super::project_view(&project),
    })))
}
