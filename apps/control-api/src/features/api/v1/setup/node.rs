use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::DatabaseConnection;
use serde::Deserialize;

use crate::{
    domain::{
        nodes::{self, CreateNodeParams},
        settings,
    },
    infra::error::{AppError, ok_response},
    init,
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/node", axum::routing::post(handler))
}

fn setup_database<'a>(
    state: &'a ControlApiState,
    op: &'static str,
) -> Result<&'a DatabaseConnection, AppError> {
    state.try_database().ok_or_else(|| AppError::Validation {
        op,
        message: "database must be configured first".to_owned(),
    })
}

async fn ensure_setup_mutation_allowed(
    db: &DatabaseConnection,
    op: &'static str,
) -> Result<(), AppError> {
    if init::is_setup_finished(db)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
    {
        return Err(AppError::SetupNotAllowed {
            op,
            message: "setup has already finished".to_owned(),
        });
    }
    Ok(())
}

#[derive(Deserialize)]
struct NodeSetupRequest {
    name: Option<String>,
}

async fn handler(
    State(state): State<ControlApiState>,
    Json(body): Json<NodeSetupRequest>,
) -> Result<impl IntoResponse, AppError> {
    let _setup_guard = state.lock_setup().await;
    let db = setup_database(&state, "setup.node.database")?;
    ensure_setup_mutation_allowed(db, "setup.node.ready_mode").await?;

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
        .map_err(|source| AppError::Infrastructure {
            op: "setup.node.read_storage",
            source,
        })?
        .and_then(|setting| setting.value.as_str().map(node_work_root))
        .or_else(|| {
            let config = state.config.read().unwrap();
            Some(node_work_root(&config.storage.root))
        });

    let name = body.name.unwrap_or_else(|| "local-node".to_owned());
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(AppError::Validation {
            op: "setup.node.invalid_name",
            message: "node name must contain between 1 and 120 characters".to_owned(),
        });
    }

    let node = nodes::create_node(
        db,
        CreateNodeParams {
            name: name.to_owned(),
            region: "default".to_owned(),
            token_hash,
            storage_root: storage_root.clone(),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure {
        op: "setup.node.create",
        source,
    })?;

    // When the operator opted into the managed local node, generate its
    // config now — this is the only moment the plaintext token exists. The
    // process itself starts at setup finish, once ready mode unlocks the
    // internal API.
    let mut warnings = Vec::new();
    let mut local_node_config_generated = false;
    let (auto_start, config_path, control_api_url, config_storage_root) = {
        let config = state.config.read().unwrap();
        (
            config.node_manager.auto_start_local_node,
            config.node_manager.local_node_config.clone(),
            crate::infra::node_manager::config_file::control_api_url(
                config.server.host,
                config.server.port,
            ),
            config.storage.root.clone(),
        )
    };
    if auto_start {
        let storage_root = settings::get_setting(db, "storage.root")
            .await
            .ok()
            .flatten()
            .and_then(|setting| setting.value.as_str().map(str::to_owned))
            .unwrap_or(config_storage_root);
        match crate::infra::node_manager::config_file::generate(
            &config_path,
            &crate::infra::node_manager::config_file::GenerateParams {
                node_name: &node.name,
                region: "default",
                node_token: &token,
                control_api_url,
                storage_root: &storage_root,
            },
        ) {
            Ok(mut generated_warnings) => {
                local_node_config_generated = true;
                warnings.append(&mut generated_warnings);
            }
            Err(error) => warnings.push(format!("failed to write local node config: {error}")),
        }
    }

    Ok(ok_response(ResponseBody {
        node: ResponseBodyNodeResponse {
            id: node.id,
            name: node.name.clone(),
        },
        token,
        local_node_config_generated,
        warnings,
    }))
}

fn node_work_root(storage_root: &str) -> String {
    format!("{}/node", storage_root.trim_end_matches('/'))
}

#[derive(serde::Serialize)]
struct ResponseBodyNodeResponse {
    id: uuid::Uuid,
    name: String,
}

#[derive(serde::Serialize)]
struct ResponseBody {
    node: ResponseBodyNodeResponse,
    token: String,
    local_node_config_generated: bool,
    warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_work_root_is_derived_from_storage_root() {
        assert_eq!(node_work_root("/data"), "/data/node");
        assert_eq!(node_work_root("/srv/grass/"), "/srv/grass/node");
    }
}
