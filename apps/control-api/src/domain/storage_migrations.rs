use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, TransactionTrait,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::{nodes, storage_settings},
    infra::{
        database::entity::{
            StorageMigrationObjectStatus, StorageMigrationStatus, storage_migration_job,
            storage_migration_object,
        },
        storage::{
            StorageConfig, StorageCredentials, build_backend, copy_and_verify, list_managed_backend,
        },
    },
    state::ControlApiState,
};

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub struct MigrationSweepResult {
    pub copied: u64,
    pub completed: bool,
    pub failed: bool,
}

pub async fn create(
    state: &ControlApiState,
    created_by_user_id: Uuid,
    target_config: StorageConfig,
    target_credentials: StorageCredentials,
) -> anyhow::Result<storage_migration_job::Model> {
    let db = state
        .try_database()
        .ok_or_else(|| anyhow::anyhow!("database not available"))?;
    target_config.validate()?;
    let platform_secret = state.config.read().unwrap().secrets.secret_key.clone();
    let source =
        storage_settings::load_or_seed(db, &state.storage.config().local_root, &platform_secret)
            .await?;
    if source.config == target_config && source.credentials == target_credentials {
        anyhow::bail!("target storage configuration is already active");
    }
    let target_credentials_envelope = target_credentials
        .is_configured()
        .then(|| storage_settings::encrypt_credentials(&platform_secret, &target_credentials))
        .transpose()?;
    let now = OffsetDateTime::now_utc();
    let job = storage_migration_job::ActiveModel {
        id: Set(Uuid::now_v7()),
        status: Set(StorageMigrationStatus::Pending),
        source_config: Set(serde_json::to_value(source.config)?),
        source_credentials: Set(source.credentials_envelope),
        target_config: Set(serde_json::to_value(target_config)?),
        target_credentials: Set(target_credentials_envelope),
        copied_objects: Set(0),
        copied_bytes: Set(0),
        total_objects: Set(None),
        total_bytes: Set(None),
        last_error: Set(None),
        created_by_user_id: Set(Some(created_by_user_id)),
        created_at: Set(now),
        started_at: Set(None),
        finished_at: Set(None),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;
    state.storage.mark_maintenance();
    Ok(job)
}

pub async fn latest(
    db: &DatabaseConnection,
) -> anyhow::Result<Option<storage_migration_job::Model>> {
    Ok(storage_migration_job::Entity::find()
        .order_by_desc(storage_migration_job::Column::CreatedAt)
        .one(db)
        .await?)
}

pub async fn has_active(db: &DatabaseConnection) -> anyhow::Result<bool> {
    Ok(active(db).await?.is_some())
}

async fn active(db: &DatabaseConnection) -> anyhow::Result<Option<storage_migration_job::Model>> {
    Ok(storage_migration_job::Entity::find()
        .filter(storage_migration_job::Column::Status.is_in([
            StorageMigrationStatus::Pending,
            StorageMigrationStatus::Running,
        ]))
        .order_by_asc(storage_migration_job::Column::CreatedAt)
        .one(db)
        .await?)
}

pub async fn sweep(state: &ControlApiState) -> anyhow::Result<MigrationSweepResult> {
    let Some(db) = state.try_database() else {
        return Ok(MigrationSweepResult::default());
    };
    let Some(job) = active(db).await? else {
        return Ok(MigrationSweepResult::default());
    };
    let maintenance_guard = state.storage.enter_maintenance().await;
    let result = process_job(state, db, job.clone()).await;
    drop(maintenance_guard);
    state.storage.leave_maintenance();
    match result {
        Ok(result) => Ok(result),
        Err(error) => {
            record_job_failure(db, job.id, &error).await?;
            Err(error)
        }
    }
}

async fn process_job(
    state: &ControlApiState,
    db: &DatabaseConnection,
    mut job: storage_migration_job::Model,
) -> anyhow::Result<MigrationSweepResult> {
    let platform_secret = state.config.read().unwrap().secrets.secret_key.clone();
    let source_config: StorageConfig = serde_json::from_value(job.source_config.clone())?;
    let target_config: StorageConfig = serde_json::from_value(job.target_config.clone())?;
    let source_credentials = decrypt_optional(&platform_secret, job.source_credentials.as_ref())?;
    let target_credentials = decrypt_optional(&platform_secret, job.target_credentials.as_ref())?;
    let source = build_backend(&source_config, &source_credentials)?;
    let target = build_backend(&target_config, &target_credentials)?;

    if matches!(job.status, StorageMigrationStatus::Pending) {
        let now = OffsetDateTime::now_utc();
        let mut active: storage_migration_job::ActiveModel = job.into();
        active.status = Set(StorageMigrationStatus::Running);
        active.started_at = Set(Some(now));
        active.updated_at = Set(now);
        job = active.update(db).await?;
    }

    reset_interrupted_objects(db, job.id).await?;
    ensure_object_manifest(db, &job, &source).await?;
    let mut copied = 0_u64;
    while let Some(item) = next_object(db, job.id).await? {
        let item = mark_object_running(db, item).await?;
        let source_size = item.source_size;
        match copy_and_verify(&source, &target, &item.object_key).await {
            Ok((size_bytes, checksum)) if i64::try_from(size_bytes).ok() == Some(source_size) => {
                mark_object_succeeded(db, item, &checksum, source_size).await?;
                copied += 1;
            }
            Ok(_) => {
                let error =
                    anyhow::anyhow!("source size changed while migrating {}", item.object_key);
                mark_object_failed(db, item, &error).await?;
                return Err(error);
            }
            Err(error) => {
                let error = anyhow::Error::from(error);
                mark_object_failed(db, item, &error).await?;
                return Err(error);
            }
        }
    }

    ensure_all_objects_succeeded(db, job.id).await?;

    let transaction = db.begin().await?;
    storage_settings::save_raw(
        &transaction,
        &target_config,
        job.target_credentials.as_ref(),
    )
    .await?;
    nodes::update_work_roots(
        &transaction,
        &nodes::work_root_for_storage(&target_config.local_root),
    )
    .await?;
    let current = storage_migration_job::Entity::find_by_id(job.id)
        .one(&transaction)
        .await?
        .ok_or_else(|| anyhow::anyhow!("storage migration job disappeared"))?;
    let now = OffsetDateTime::now_utc();
    let mut active: storage_migration_job::ActiveModel = current.into();
    active.status = Set(StorageMigrationStatus::Succeeded);
    active.last_error = Set(None);
    active.finished_at = Set(Some(now));
    active.updated_at = Set(now);
    active.update(&transaction).await?;
    transaction.commit().await?;
    let local_node_config = state
        .config
        .read()
        .unwrap()
        .node_manager
        .local_node_config
        .clone();
    if let Err(error) = crate::infra::node_manager::config_file::update_storage_root(
        &local_node_config,
        &target_config.local_root,
    ) {
        tracing::warn!(
            operation = "storage.migration.update_local_node_config",
            %error,
            "failed to update generated local node config"
        );
    }
    state.storage.replace_backend(target_config, target);
    Ok(MigrationSweepResult {
        copied,
        completed: true,
        failed: false,
    })
}

fn decrypt_optional(
    platform_secret: &str,
    envelope: Option<&serde_json::Value>,
) -> anyhow::Result<StorageCredentials> {
    envelope
        .map(|value| storage_settings::decrypt_credentials(platform_secret, value))
        .transpose()
        .map(|credentials| credentials.unwrap_or_default())
}

async fn ensure_object_manifest(
    db: &DatabaseConnection,
    job: &storage_migration_job::Model,
    source: &std::sync::Arc<dyn crate::infra::storage::ObjectStorage>,
) -> anyhow::Result<()> {
    if storage_migration_object::Entity::find()
        .filter(storage_migration_object::Column::JobId.eq(job.id))
        .one(db)
        .await?
        .is_some()
    {
        return Ok(());
    }
    let objects = list_managed_backend(source).await?;
    let total_objects = i64::try_from(objects.len())?;
    let total_bytes = objects.iter().try_fold(0_i64, |total, object| {
        let size = i64::try_from(object.size_bytes)?;
        total
            .checked_add(size)
            .ok_or_else(|| anyhow::anyhow!("storage migration byte total overflow"))
    })?;
    let transaction = db.begin().await?;
    let now = OffsetDateTime::now_utc();
    for object in objects {
        storage_migration_object::ActiveModel {
            job_id: Set(job.id),
            object_key: Set(object.key),
            source_size: Set(i64::try_from(object.size_bytes)?),
            status: Set(StorageMigrationObjectStatus::Pending),
            checksum_sha256: Set(None),
            attempt_count: Set(0),
            last_error: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&transaction)
        .await?;
    }
    let current = storage_migration_job::Entity::find_by_id(job.id)
        .one(&transaction)
        .await?
        .ok_or_else(|| anyhow::anyhow!("storage migration job disappeared"))?;
    let mut active: storage_migration_job::ActiveModel = current.into();
    active.total_objects = Set(Some(total_objects));
    active.total_bytes = Set(Some(total_bytes));
    active.updated_at = Set(now);
    active.update(&transaction).await?;
    transaction.commit().await?;
    Ok(())
}

async fn reset_interrupted_objects(db: &DatabaseConnection, job_id: Uuid) -> anyhow::Result<()> {
    let items = storage_migration_object::Entity::find()
        .filter(storage_migration_object::Column::JobId.eq(job_id))
        .filter(storage_migration_object::Column::Status.is_in([
            StorageMigrationObjectStatus::Running,
            StorageMigrationObjectStatus::Failed,
        ]))
        .all(db)
        .await?;
    for item in items {
        let mut active: storage_migration_object::ActiveModel = item.into();
        active.status = Set(StorageMigrationObjectStatus::Pending);
        active.last_error = Set(None);
        active.updated_at = Set(OffsetDateTime::now_utc());
        active.update(db).await?;
    }
    Ok(())
}

async fn next_object(
    db: &DatabaseConnection,
    job_id: Uuid,
) -> anyhow::Result<Option<storage_migration_object::Model>> {
    Ok(storage_migration_object::Entity::find()
        .filter(storage_migration_object::Column::JobId.eq(job_id))
        .filter(storage_migration_object::Column::Status.eq(StorageMigrationObjectStatus::Pending))
        .order_by_asc(storage_migration_object::Column::ObjectKey)
        .one(db)
        .await?)
}

async fn ensure_all_objects_succeeded(db: &DatabaseConnection, job_id: Uuid) -> anyhow::Result<()> {
    if let Some(item) = storage_migration_object::Entity::find()
        .filter(storage_migration_object::Column::JobId.eq(job_id))
        .filter(
            storage_migration_object::Column::Status.ne(StorageMigrationObjectStatus::Succeeded),
        )
        .order_by_asc(storage_migration_object::Column::ObjectKey)
        .one(db)
        .await?
    {
        anyhow::bail!(
            "storage migration has incomplete object {} ({:?})",
            item.object_key,
            item.status
        );
    }
    Ok(())
}

async fn mark_object_running(
    db: &DatabaseConnection,
    item: storage_migration_object::Model,
) -> anyhow::Result<storage_migration_object::Model> {
    let attempt_count = item.attempt_count.saturating_add(1);
    let mut active: storage_migration_object::ActiveModel = item.into();
    active.status = Set(StorageMigrationObjectStatus::Running);
    active.attempt_count = Set(attempt_count);
    active.last_error = Set(None);
    active.updated_at = Set(OffsetDateTime::now_utc());
    Ok(active.update(db).await?)
}

async fn mark_object_succeeded(
    db: &DatabaseConnection,
    item: storage_migration_object::Model,
    checksum: &str,
    source_size: i64,
) -> anyhow::Result<()> {
    let transaction = db.begin().await?;
    let job_id = item.job_id;
    let mut active: storage_migration_object::ActiveModel = item.into();
    active.status = Set(StorageMigrationObjectStatus::Succeeded);
    active.checksum_sha256 = Set(Some(checksum.to_owned()));
    active.last_error = Set(None);
    active.updated_at = Set(OffsetDateTime::now_utc());
    active.update(&transaction).await?;

    let job = storage_migration_job::Entity::find_by_id(job_id)
        .one(&transaction)
        .await?
        .ok_or_else(|| anyhow::anyhow!("storage migration job disappeared"))?;
    let copied_objects = job.copied_objects.saturating_add(1);
    let copied_bytes = job.copied_bytes.saturating_add(source_size);
    let mut active: storage_migration_job::ActiveModel = job.into();
    active.copied_objects = Set(copied_objects);
    active.copied_bytes = Set(copied_bytes);
    active.updated_at = Set(OffsetDateTime::now_utc());
    active.update(&transaction).await?;
    transaction.commit().await?;
    Ok(())
}

async fn mark_object_failed(
    db: &DatabaseConnection,
    item: storage_migration_object::Model,
    error: &anyhow::Error,
) -> anyhow::Result<()> {
    let mut active: storage_migration_object::ActiveModel = item.into();
    active.status = Set(StorageMigrationObjectStatus::Failed);
    active.last_error = Set(Some(bounded_error(error)));
    active.updated_at = Set(OffsetDateTime::now_utc());
    active.update(db).await?;
    Ok(())
}

async fn record_job_failure(
    db: &DatabaseConnection,
    job_id: Uuid,
    error: &anyhow::Error,
) -> anyhow::Result<()> {
    let Some(job) = storage_migration_job::Entity::find_by_id(job_id)
        .one(db)
        .await?
    else {
        return Ok(());
    };
    if matches!(job.status, StorageMigrationStatus::Succeeded) {
        return Ok(());
    }
    let now = OffsetDateTime::now_utc();
    let mut active: storage_migration_job::ActiveModel = job.into();
    active.status = Set(StorageMigrationStatus::Failed);
    active.last_error = Set(Some(bounded_error(error)));
    active.finished_at = Set(Some(now));
    active.updated_at = Set(now);
    active.update(db).await?;
    Ok(())
}

fn bounded_error(error: &anyhow::Error) -> String {
    error.to_string().chars().take(1000).collect()
}

pub fn status_value(status: &StorageMigrationStatus) -> &'static str {
    match status {
        StorageMigrationStatus::Pending => "pending",
        StorageMigrationStatus::Running => "running",
        StorageMigrationStatus::Succeeded => "succeeded",
        StorageMigrationStatus::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{DbBackend, MockDatabase};

    use super::*;
    use crate::infra::{
        config::ControlApiConfig,
        database::entity::{SystemSettingValueKind, storage_migration_object, system_setting},
        node_manager::config_file::{self, GenerateParams},
    };

    fn setting(key: &str) -> system_setting::Model {
        system_setting::Model {
            id: Uuid::now_v7(),
            key: key.to_owned(),
            value_kind: SystemSettingValueKind::Json,
            value: serde_json::Value::Null,
            is_secret: false,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    fn job(
        id: Uuid,
        source_config: &StorageConfig,
        target_config: &StorageConfig,
    ) -> storage_migration_job::Model {
        let now = OffsetDateTime::UNIX_EPOCH;
        storage_migration_job::Model {
            id,
            status: StorageMigrationStatus::Running,
            source_config: serde_json::to_value(source_config).unwrap(),
            source_credentials: None,
            target_config: serde_json::to_value(target_config).unwrap(),
            target_credentials: None,
            copied_objects: 0,
            copied_bytes: 0,
            total_objects: None,
            total_bytes: None,
            last_error: None,
            created_by_user_id: None,
            created_at: now,
            started_at: Some(now),
            finished_at: None,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn completing_local_root_migration_updates_nodes_and_generated_config() {
        let source_root =
            std::env::temp_dir().join(format!("grass-migration-source-{}", Uuid::now_v7()));
        let target_root =
            std::env::temp_dir().join(format!("grass-migration-target-{}", Uuid::now_v7()));
        let config_path =
            std::env::temp_dir().join(format!("grass-migration-node-{}.toml", Uuid::now_v7()));
        let node_config = GenerateParams {
            node_name: "local-node",
            node_token: "node-token",
            control_api_url: "http://127.0.0.1:7817".to_owned(),
            storage_root: source_root.to_str().unwrap(),
        };
        config_file::generate(config_path.to_str().unwrap(), &node_config).unwrap();

        let source_config = StorageConfig::local(source_root.to_string_lossy());
        let target_config = StorageConfig::local(target_root.to_string_lossy());
        let migration_job = job(Uuid::now_v7(), &source_config, &target_config);
        let database = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([Vec::<storage_migration_object::Model>::new()])
            .append_query_results([Vec::<storage_migration_object::Model>::new()])
            .append_query_results([vec![migration_job.clone()]])
            .append_query_results([vec![migration_job.clone()]])
            .append_query_results([Vec::<storage_migration_object::Model>::new()])
            .append_query_results([Vec::<storage_migration_object::Model>::new()])
            .append_query_results([Vec::<system_setting::Model>::new()])
            .append_query_results([vec![setting(storage_settings::CONFIG_KEY)]])
            .append_query_results([Vec::<system_setting::Model>::new()])
            .append_query_results([vec![setting(storage_settings::CREDENTIALS_KEY)]])
            .append_query_results([Vec::<system_setting::Model>::new()])
            .append_query_results([vec![setting(storage_settings::LEGACY_ROOT_KEY)]])
            .append_query_results([vec![migration_job.clone()]])
            .append_query_results([vec![migration_job.clone()]])
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
        let mut config = ControlApiConfig::default();
        config.node_manager.local_node_config = config_path.to_string_lossy().into_owned();
        let state = ControlApiState::new(config, "unused.toml");
        state.database.set(database.clone()).unwrap();

        let result = process_job(&state, &database, migration_job.clone()).await;

        assert!(result.is_ok(), "migration completion failed: {result:?}");
        let statements = format!("{:?}", database.into_transaction_log());
        assert!(statements.contains("UPDATE \\\"nodes\\\""), "{statements}");
        let value: toml::Value = std::fs::read_to_string(&config_path)
            .unwrap()
            .parse()
            .unwrap();
        let expected_work_root = format!("{}/node", target_root.display());
        assert_eq!(
            value["node"]["work_root"].as_str(),
            Some(expected_work_root.as_str())
        );

        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir_all(source_root);
        let _ = std::fs::remove_dir_all(target_root);
    }

    #[tokio::test]
    async fn resets_failed_objects_after_an_interrupted_active_job() {
        let job_id = Uuid::now_v7();
        let now = OffsetDateTime::UNIX_EPOCH;
        let object =
            |key: &str, status: StorageMigrationObjectStatus| storage_migration_object::Model {
                job_id,
                object_key: key.to_owned(),
                source_size: 1,
                status: status.clone(),
                checksum_sha256: None,
                attempt_count: 1,
                last_error: (status == StorageMigrationObjectStatus::Failed)
                    .then(|| "copy failed".to_owned()),
                created_at: now,
                updated_at: now,
            };
        let failed = object("deployments/failed", StorageMigrationObjectStatus::Failed);
        let running = object("deployments/running", StorageMigrationObjectStatus::Running);
        let database = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([vec![failed.clone(), running.clone()]])
            .append_exec_results([
                sea_orm::MockExecResult {
                    last_insert_id: 0,
                    rows_affected: 1,
                },
                sea_orm::MockExecResult {
                    last_insert_id: 0,
                    rows_affected: 1,
                },
            ])
            .append_query_results([vec![failed], vec![running]])
            .into_connection();

        reset_interrupted_objects(&database, job_id).await.unwrap();

        let statements = format!("{:?}", database.into_transaction_log());
        assert_eq!(statements.matches("UPDATE").count(), 2, "{statements}");
        assert!(statements.contains("status"), "{statements}");
    }

    #[tokio::test]
    async fn object_success_and_progress_commit_in_one_transaction() {
        let source_config = StorageConfig::local("/srv/source");
        let target_config = StorageConfig::local("/srv/target");
        let migration_job = job(Uuid::now_v7(), &source_config, &target_config);
        let now = OffsetDateTime::UNIX_EPOCH;
        let object = storage_migration_object::Model {
            job_id: migration_job.id,
            object_key: "deployments/project/deployment/grass-output.zip".to_owned(),
            source_size: 512,
            status: StorageMigrationObjectStatus::Running,
            checksum_sha256: None,
            attempt_count: 1,
            last_error: None,
            created_at: now,
            updated_at: now,
        };
        let database = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([vec![object.clone()]])
            .append_query_results([vec![migration_job.clone()]])
            .append_query_results([vec![migration_job]])
            .append_exec_results([
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

        mark_object_succeeded(&database, object, "checksum", 512)
            .await
            .unwrap();

        let transactions = database.into_transaction_log();
        assert_eq!(transactions.len(), 1, "{transactions:?}");
        let statements = format!("{transactions:?}");
        assert!(
            statements.contains("UPDATE \\\"storage_migration_objects\\\""),
            "{statements}"
        );
        assert!(
            statements.contains("UPDATE \\\"storage_migration_jobs\\\""),
            "{statements}"
        );
    }
}
