use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::{
        nodes::{self, CreateNodeParams},
        settings,
    },
    infra::error::{AppError, ok_response},
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct NodeSetupRequest {
    pub name: Option<String>,
}

pub async fn handler(
    State(state): State<ControlApiState>,
    Json(body): Json<NodeSetupRequest>,
) -> Result<impl IntoResponse, AppError> {
    let db = super::setup_database(&state, "setup.node.database")?;
    super::ensure_setup_mutation_allowed(db, "setup.node.ready_mode").await?;

    if nodes::any_node_exists(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.node.check_existing",
            source,
        })?
    {
        return Err(AppError::Conflict {
            op: "setup.node.already_exists",
            message: "a node already exists".to_owned(),
        });
    }

    let token = grass_token::generate_token();
    let token_hash = grass_token::hash_token(&token);
    let storage_root = settings::get_setting(db, "storage.root")
        .await
        .ok()
        .flatten()
        .and_then(|setting| setting.value.as_str().map(str::to_owned));

    let node = nodes::create_node(
        db,
        CreateNodeParams {
            name: body.name.unwrap_or_else(|| "local-node".to_owned()),
            token_hash,
            storage_root,
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure {
        op: "setup.node.create",
        source,
    })?;

    Ok(ok_response(json!({
        "node": { "id": node.id, "name": node.name },
        "token": token,
    })))
}
