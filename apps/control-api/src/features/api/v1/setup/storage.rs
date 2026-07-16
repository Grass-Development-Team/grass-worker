use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::TransactionTrait;
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

use crate::{
    domain::{nodes, settings},
    infra::error::{AppError, ok_response},
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct StorageSetupRequest {
    pub root: Option<String>,
}

pub async fn handler(
    State(state): State<ControlApiState>,
    Json(body): Json<StorageSetupRequest>,
) -> Result<impl IntoResponse, AppError> {
    let _setup_guard = state.lock_setup().await;
    let db = super::setup_database(&state, "setup.storage.database")?;
    super::ensure_setup_mutation_allowed(db, "setup.storage.ready_mode").await?;

    let root = validate_storage_root(body.root.as_deref().unwrap_or("/data"))?;
    let original_config = crate::infra::config::ControlApiConfig::load_persisted(
        state.config_path(),
    )
    .map_err(|error| AppError::Infrastructure {
        op: "setup.storage.load_config",
        source: anyhow::anyhow!(error),
    })?;
    let mut persisted_config = original_config.clone();
    persisted_config.ensure_secret_key();
    persisted_config.storage.root = root.clone();
    let mut updated_config = state.config.read().unwrap().clone();
    updated_config.storage.root = root.clone();
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.storage.begin_transaction",
            source: source.into(),
        })?;
    settings::set_string(&transaction, "storage.root", &root)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.storage.save",
            source,
        })?;
    nodes::update_work_roots(&transaction, &super::node::node_work_root(&root))
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.storage.update_node",
            source,
        })?;

    persisted_config
        .save(state.config_path())
        .map_err(|error| AppError::Infrastructure {
            op: "setup.storage.save_config",
            source: anyhow::anyhow!(error),
        })?;
    if let Err(source) = transaction.commit().await {
        if let Err(restore_error) = original_config.save(state.config_path()) {
            tracing::error!(
                operation = "setup.storage.restore_config",
                %restore_error,
                "failed to restore bootstrap config after transaction failure"
            );
        }
        return Err(AppError::Infrastructure {
            op: "setup.storage.commit",
            source: source.into(),
        });
    }
    *state.config.write().unwrap() = updated_config;

    Ok(ok_response(json!({ "configured": true, "root": root })))
}

fn validate_storage_root(root: &str) -> Result<String, AppError> {
    let root = root.trim().trim_end_matches('/');
    if root.is_empty() || !Path::new(root).is_absolute() {
        return Err(AppError::Validation {
            op: "setup.storage.invalid_root",
            message: "storage root must be a non-empty absolute path".to_owned(),
        });
    }
    Ok(root.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_root_must_be_absolute_and_normalized() {
        assert_eq!(validate_storage_root(" /srv/grass ").unwrap(), "/srv/grass");
        assert!(validate_storage_root("relative/path").is_err());
        assert!(validate_storage_root("   ").is_err());
    }
}
