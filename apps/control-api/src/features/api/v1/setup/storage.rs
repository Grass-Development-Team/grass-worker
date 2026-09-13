use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::{DatabaseConnection, TransactionTrait};
use serde::Deserialize;

use crate::{
    domain::{nodes, storage_settings},
    infra::{
        error::{AppError, ok_response},
        storage::{StorageBackendKind, StorageConfig, StorageCredentials, build_backend},
    },
    init,
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/storage", axum::routing::post(handler))
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

#[derive(Debug, Default, Deserialize)]
struct StorageSetupRequest {
    #[serde(default)]
    backend: Option<String>,
    #[serde(default)]
    root: Option<String>,
    #[serde(flatten)]
    options: storage_settings::StorageOptions,
}

async fn handler(
    State(state): State<ControlApiState>,
    Json(body): Json<StorageSetupRequest>,
) -> Result<impl IntoResponse, AppError> {
    let _setup_guard = state.lock_setup().await;
    let db = setup_database(&state, "setup.storage.database")?;
    ensure_setup_mutation_allowed(db, "setup.storage.ready_mode").await?;

    let (config, credentials) = prepare(body)?;
    let credentials_configured = credentials.is_configured();
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

    Ok(ok_response(ResponseBody {
        configured: true,
        storage: storage_view(&config, credentials_configured),
    }))
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

fn node_work_root(storage_root: &str) -> String {
    nodes::work_root_for_storage(storage_root)
}

fn prepare(body: StorageSetupRequest) -> Result<(StorageConfig, StorageCredentials), AppError> {
    let backend = body
        .backend
        .as_deref()
        .unwrap_or("local")
        .parse::<StorageBackendKind>()
        .map_err(|_| AppError::Validation {
            op: "setup.storage.invalid_backend",
            message: "backend must be local, s3, minio or r2".to_owned(),
        })?;
    body.options
        .resolve(backend, body.root.as_deref().unwrap_or("/data"))
        .map_err(|source| AppError::Validation {
            op: "setup.storage.invalid_config",
            message: source.to_string(),
        })
}

#[derive(serde::Serialize)]
struct StorageResponse {
    backend: &'static str,
    local_root: String,
    endpoint: String,
    region: String,
    bucket: String,
    prefix: String,
    force_path_style: bool,
    allow_http: bool,
    credentials_configured: bool,
}
fn storage_view(
    config: &crate::infra::storage::StorageConfig,
    credentials_configured: bool,
) -> StorageResponse {
    StorageResponse {
        backend: config.backend.as_str(),
        local_root: config.local_root.clone(),
        endpoint: config.endpoint.clone(),
        region: config.region.clone(),
        bucket: config.bucket.clone(),
        prefix: config.prefix.clone(),
        force_path_style: config.force_path_style,
        allow_http: config.allow_http,
        credentials_configured,
    }
}

#[derive(serde::Serialize)]
struct ResponseBody {
    configured: bool,
    storage: StorageResponse,
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::storage_settings;
    use crate::infra::database::entity::SystemSettingValueKind;
    use crate::infra::database::entity::system_setting;
    use crate::infra::storage::StorageConfig;
    use sea_orm::DbBackend;
    use sea_orm::MockDatabase;
    use serde_json::json;
    #[test]
    fn setup_root_compatibility_and_precedence_are_preserved() {
        for (input, expected) in [
            (json!({}), "/data"),
            (json!({"root":" /srv/legacy/ "}), "/srv/legacy"),
            (
                json!({"root":"/srv/legacy", "local_root":"/srv/current"}),
                "/srv/current",
            ),
        ] {
            let (config, _) = prepare(serde_json::from_value(input).unwrap()).unwrap();
            assert_eq!(config.local_root, expected);
        }
        for root in ["relative/path", "   "] {
            assert!(prepare(serde_json::from_value(json!({"root":root})).unwrap()).is_err());
        }
    }

    #[test]
    fn node_work_root_stays_local_for_remote_backends() {
        assert_eq!(node_work_root("/srv/grass"), "/srv/grass/node");
    }

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
