use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::{DatabaseConnection, TransactionTrait};
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::{nodes, storage_settings},
    infra::{
        error::{AppError, ok_response},
        storage::{StorageBackendKind, StorageConfig, StorageCredentials, build_backend},
    },
    state::ControlApiState,
};

#[derive(Debug, Default, Deserialize)]
pub struct StorageSetupRequest {
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub local_root: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub bucket: Option<String>,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub force_path_style: Option<bool>,
    #[serde(default)]
    pub allow_http: Option<bool>,
    #[serde(default)]
    pub access_key_id: Option<String>,
    #[serde(default)]
    pub secret_access_key: Option<String>,
    #[serde(default)]
    pub session_token: Option<String>,
}

pub async fn handler(
    State(state): State<ControlApiState>,
    Json(body): Json<StorageSetupRequest>,
) -> Result<impl IntoResponse, AppError> {
    let _setup_guard = state.lock_setup().await;
    let db = super::setup_database(&state, "setup.storage.database")?;
    super::ensure_setup_mutation_allowed(db, "setup.storage.ready_mode").await?;

    let root = body
        .local_root
        .or(body.root)
        .unwrap_or_else(|| "/data".to_owned());
    let backend = body
        .backend
        .as_deref()
        .unwrap_or("local")
        .parse::<StorageBackendKind>()
        .map_err(|_| AppError::Validation {
            op: "setup.storage.invalid_backend",
            message: "backend must be local, s3, minio or r2".to_owned(),
        })?;
    let config = StorageConfig {
        backend,
        local_root: validate_storage_root(&root)?,
        endpoint: body.endpoint.unwrap_or_default().trim().to_owned(),
        region: body
            .region
            .unwrap_or_else(|| backend.default_region().to_owned())
            .trim()
            .to_owned(),
        bucket: body.bucket.unwrap_or_default().trim().to_owned(),
        prefix: body.prefix.unwrap_or_default().trim_matches('/').to_owned(),
        force_path_style: body
            .force_path_style
            .unwrap_or(matches!(backend, StorageBackendKind::Minio)),
        allow_http: body
            .allow_http
            .unwrap_or(matches!(backend, StorageBackendKind::Minio)),
    };
    let credentials = StorageCredentials {
        access_key_id: body.access_key_id.filter(|value| !value.trim().is_empty()),
        secret_access_key: body
            .secret_access_key
            .filter(|value| !value.trim().is_empty()),
        session_token: body.session_token.filter(|value| !value.trim().is_empty()),
    };
    let credentials_configured = credentials.is_configured();
    config.validate().map_err(|source| AppError::Validation {
        op: "setup.storage.invalid_config",
        message: source.to_string(),
    })?;
    let backend_instance =
        build_backend(&config, &credentials).map_err(|source| AppError::Validation {
            op: "setup.storage.build_backend",
            message: source.to_string(),
        })?;
    backend_instance
        .probe()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.storage.test_backend",
            source: source.into(),
        })?;

    let platform_secret = state.config.read().unwrap().secrets.secret_key.clone();
    let credentials_envelope = credentials
        .is_configured()
        .then(|| storage_settings::encrypt_credentials(&platform_secret, &credentials))
        .transpose()
        .map_err(|source| AppError::Infrastructure {
            op: "setup.storage.encrypt_credentials",
            source,
        })?;
    persist_configuration(db, &config, credentials_envelope.as_ref())
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.storage.save",
            source,
        })?;
    state
        .storage
        .replace_backend(config.clone(), backend_instance);

    let local_node_config = state
        .config
        .read()
        .unwrap()
        .node_manager
        .local_node_config
        .clone();
    if let Err(error) = crate::infra::node_manager::config_file::update_storage_root(
        &local_node_config,
        &config.local_root,
    ) {
        tracing::warn!(
            operation = "setup.storage.update_local_node_config",
            %error,
            "failed to update generated local node config"
        );
    }

    Ok(ok_response(json!({
        "configured": true,
        "storage": storage_settings::public_config(&config, credentials_configured),
    })))
}

async fn persist_configuration(
    db: &DatabaseConnection,
    config: &StorageConfig,
    credentials_envelope: Option<&serde_json::Value>,
) -> anyhow::Result<()> {
    let transaction = db.begin().await?;
    storage_settings::save_raw(&transaction, config, credentials_envelope).await?;
    nodes::update_work_roots(&transaction, &node_work_root(&config.local_root)).await?;
    transaction.commit().await?;
    Ok(())
}

pub(super) fn node_work_root(storage_root: &str) -> String {
    nodes::work_root_for_storage(storage_root)
}

fn validate_storage_root(root: &str) -> Result<String, AppError> {
    let root = root.trim().trim_end_matches('/');
    if root.is_empty() || !std::path::Path::new(root).is_absolute() {
        return Err(AppError::Validation {
            op: "setup.storage.invalid_root",
            message: "storage root must be a non-empty absolute path".to_owned(),
        });
    }
    Ok(root.to_owned())
}

#[cfg(test)]
mod tests {
    use sea_orm::{DbBackend, MockDatabase};

    use super::*;
    use crate::infra::database::entity::{SystemSettingValueKind, system_setting};

    fn setting(key: &str) -> system_setting::Model {
        let now = time::OffsetDateTime::UNIX_EPOCH;
        system_setting::Model {
            id: uuid::Uuid::now_v7(),
            key: key.to_owned(),
            value_kind: SystemSettingValueKind::Json,
            value: serde_json::Value::Null,
            is_secret: false,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn storage_root_must_be_absolute_and_normalized() {
        assert_eq!(validate_storage_root(" /srv/grass ").unwrap(), "/srv/grass");
        assert!(validate_storage_root("relative/path").is_err());
        assert!(validate_storage_root("   ").is_err());
    }

    #[test]
    fn node_work_root_stays_local_for_remote_backends() {
        assert_eq!(node_work_root("/srv/grass"), "/srv/grass/node");
    }

    #[tokio::test]
    async fn setup_persists_storage_and_node_roots_in_one_transaction() {
        let config = StorageConfig::local("/srv/grass");
        let database = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([Vec::<system_setting::Model>::new()])
            .append_query_results([vec![setting(storage_settings::CONFIG_KEY)]])
            .append_query_results([Vec::<system_setting::Model>::new()])
            .append_query_results([vec![setting(storage_settings::CREDENTIALS_KEY)]])
            .append_query_results([Vec::<system_setting::Model>::new()])
            .append_query_results([vec![setting(storage_settings::LEGACY_ROOT_KEY)]])
            .append_exec_results([
                sea_orm::MockExecResult {
                    last_insert_id: 0,
                    rows_affected: 1,
                },
                sea_orm::MockExecResult {
                    last_insert_id: 0,
                    rows_affected: 1,
                },
                sea_orm::MockExecResult {
                    last_insert_id: 0,
                    rows_affected: 1,
                },
                sea_orm::MockExecResult {
                    last_insert_id: 0,
                    rows_affected: 1,
                },
            ])
            .into_connection();

        persist_configuration(&database, &config, None)
            .await
            .unwrap();

        let transactions = database.into_transaction_log();
        assert_eq!(transactions.len(), 1, "{transactions:?}");
        let statements = format!("{transactions:?}");
        assert!(statements.contains("UPDATE \\\"nodes\\\""), "{statements}");
    }
}
