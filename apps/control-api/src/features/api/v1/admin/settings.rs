use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::{ConnectionTrait, DatabaseTransaction, TransactionTrait};
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        nodes, settings,
    },
    infra::{
        database::entity::{AuditEventResult, SystemSettingValueKind},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

const SITE_NAME_KEY: &str = "site.name";
const SITE_URL_KEY: &str = "site.url";
const PUBLIC_BASE_URL_KEY: &str = "site.public_base_url";
const STORAGE_ROOT_KEY: &str = "storage.root";
const SIGNUP_POLICY_KEY: &str = "signup.policy";
const REVIEW_POLICY_KEY: &str = "release_review_policy.default";

async fn setting_string<C: ConnectionTrait>(
    db: &C,
    key: &str,
    op: &'static str,
) -> Result<Option<String>, AppError> {
    Ok(settings::get_setting(db, key)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .and_then(|setting| setting.value.as_str().map(str::to_owned)))
}

/// GET /api/v1/admin/settings — the editable platform base configuration.
pub async fn get(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.settings.get";
    let db = super::database(&state, OP)?;

    let review = settings::get_setting(db, REVIEW_POLICY_KEY)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .map(|setting| setting.value)
        .unwrap_or_else(|| json!({}));

    Ok(ok_response(json!({
        "site": {
            "name": setting_string(db, SITE_NAME_KEY, OP).await?,
            "url": setting_string(db, SITE_URL_KEY, OP).await?,
            "public_base_url": setting_string(db, PUBLIC_BASE_URL_KEY, OP).await?,
        },
        "storage": {
            "root": setting_string(db, STORAGE_ROOT_KEY, OP)
                .await?
                .unwrap_or_else(|| state.config.read().unwrap().storage.root.clone()),
        },
        "signup": {
            "policy": setting_string(db, SIGNUP_POLICY_KEY, OP)
                .await?
                .unwrap_or_else(|| "open".to_owned()),
        },
        "review": {
            "production": review.get("production").and_then(|v| v.as_str()).unwrap_or("manual"),
            "preview": review.get("preview").and_then(|v| v.as_str()).unwrap_or("auto"),
        },
    })))
}

#[derive(Deserialize)]
pub struct UpdateSettingsRequest {
    pub site_name: Option<String>,
    pub site_url: Option<String>,
    pub public_base_url: Option<String>,
    pub storage_root: Option<String>,
    pub signup_policy: Option<String>,
    pub review_production: Option<String>,
    pub review_preview: Option<String>,
}

struct PreparedSettingsUpdate {
    site_name: Option<String>,
    site_url: Option<String>,
    public_base_url: Option<String>,
    storage_root: Option<String>,
    signup_policy: Option<String>,
    review_production: Option<String>,
    review_preview: Option<String>,
}

struct PreparedStorageConfigUpdate {
    original: crate::infra::config::ControlApiConfig,
    persisted: crate::infra::config::ControlApiConfig,
    runtime: crate::infra::config::ControlApiConfig,
    local_node_config: String,
}

fn validate_review_mode(value: &str, op: &'static str) -> Result<(), AppError> {
    match value {
        "auto" | "manual" => Ok(()),
        _ => Err(AppError::Validation {
            op,
            message: format!("review mode must be auto or manual, got {value}"),
        }),
    }
}

fn prepare_settings_update(
    body: UpdateSettingsRequest,
    op: &'static str,
) -> Result<PreparedSettingsUpdate, AppError> {
    let site_name = body.site_name.map(|value| value.trim().to_owned());
    if site_name.as_deref().is_some_and(str::is_empty) {
        return Err(AppError::Validation {
            op,
            message: "site name cannot be empty".to_owned(),
        });
    }

    let site_url = body
        .site_url
        .as_deref()
        .map(|value| crate::features::api::v1::setup::site::validate_site_url(value, op))
        .transpose()?;
    let public_base_url = body
        .public_base_url
        .as_deref()
        .map(|value| crate::features::api::v1::setup::site::validate_site_url(value, op))
        .transpose()?;

    let signup_policy = body.signup_policy.map(|value| value.trim().to_owned());
    if let Some(policy) = signup_policy.as_deref()
        && !matches!(policy, "open" | "invite_only" | "closed")
    {
        return Err(AppError::Validation {
            op,
            message: format!("signup policy must be open, invite_only or closed, got {policy}"),
        });
    }

    let review_production = body.review_production.map(|value| value.trim().to_owned());
    if let Some(mode) = review_production.as_deref() {
        validate_review_mode(mode, op)?;
    }
    let review_preview = body.review_preview.map(|value| value.trim().to_owned());
    if let Some(mode) = review_preview.as_deref() {
        validate_review_mode(mode, op)?;
    }

    let storage_root = body
        .storage_root
        .map(|value| value.trim().trim_end_matches('/').to_owned());
    if let Some(root) = storage_root.as_deref()
        && (root.is_empty() || !std::path::Path::new(root).is_absolute())
    {
        return Err(AppError::Validation {
            op,
            message: "storage root must be a non-empty absolute path".to_owned(),
        });
    }

    let prepared = PreparedSettingsUpdate {
        site_name,
        site_url,
        public_base_url,
        storage_root,
        signup_policy,
        review_production,
        review_preview,
    };
    if prepared.site_name.is_none()
        && prepared.site_url.is_none()
        && prepared.public_base_url.is_none()
        && prepared.storage_root.is_none()
        && prepared.signup_policy.is_none()
        && prepared.review_production.is_none()
        && prepared.review_preview.is_none()
    {
        return Err(AppError::Validation {
            op,
            message: "nothing to update".to_owned(),
        });
    }

    Ok(prepared)
}

async fn create_settings_audit_in_transaction(
    transaction: &DatabaseTransaction,
    actor_user_id: uuid::Uuid,
    changed: Vec<&'static str>,
    before: serde_json::Map<String, serde_json::Value>,
    after: serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<()> {
    audits::create_platform_audit_event_with_changes(
        transaction,
        CreateAuditEventParams {
            actor_user_id: Some(actor_user_id),
            actor_node_id: None,
            team_id: None,
            action: "settings.updated".to_owned(),
            target_type: "settings".to_owned(),
            target_id: None,
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "changed": changed }),
        },
        json!({ "before": before, "after": after }),
    )
    .await
}

/// PATCH /api/v1/admin/settings
pub async fn update(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Json(body): Json<UpdateSettingsRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.settings.update";
    let body = prepare_settings_update(body, OP)?;
    let db = super::database(&state, OP)?;
    let storage_config_update = if let Some(root) = body.storage_root.as_deref() {
        let original = crate::infra::config::ControlApiConfig::load_persisted(state.config_path())
            .map_err(|error| AppError::Infrastructure {
                op: OP,
                source: anyhow::anyhow!(error),
            })?;
        let mut persisted = original.clone();
        persisted.ensure_secret_key();
        persisted.storage.root = root.to_owned();
        let mut runtime = state.config.read().unwrap().clone();
        runtime.storage.root = root.to_owned();
        let local_node_config = runtime.node_manager.local_node_config.clone();
        Some(PreparedStorageConfigUpdate {
            original,
            persisted,
            runtime,
            local_node_config,
        })
    } else {
        None
    };
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    let mut changed: Vec<&'static str> = Vec::new();
    let mut before = serde_json::Map::new();
    let mut after = serde_json::Map::new();

    if let Some(name) = body.site_name.as_deref() {
        before.insert(
            "site_name".to_owned(),
            setting_string(&transaction, SITE_NAME_KEY, OP)
                .await?
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        );
        settings::set_string(&transaction, SITE_NAME_KEY, name)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        after.insert("site_name".to_owned(), json!(name));
        changed.push("site_name");
    }
    if let Some(url) = body.site_url.as_deref() {
        before.insert(
            "site_url".to_owned(),
            setting_string(&transaction, SITE_URL_KEY, OP)
                .await?
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        );
        settings::set_string(&transaction, SITE_URL_KEY, url)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        after.insert("site_url".to_owned(), json!(url));
        changed.push("site_url");
    }
    if let Some(url) = body.public_base_url.as_deref() {
        before.insert(
            "public_base_url".to_owned(),
            setting_string(&transaction, PUBLIC_BASE_URL_KEY, OP)
                .await?
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null),
        );
        settings::set_string(&transaction, PUBLIC_BASE_URL_KEY, url)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        after.insert("public_base_url".to_owned(), json!(url));
        changed.push("public_base_url");
    }
    if let Some(policy) = body.signup_policy.as_deref() {
        before.insert(
            "signup_policy".to_owned(),
            json!(
                setting_string(&transaction, SIGNUP_POLICY_KEY, OP)
                    .await?
                    .unwrap_or_else(|| "open".to_owned())
            ),
        );
        settings::set_string(&transaction, SIGNUP_POLICY_KEY, policy)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        after.insert("signup_policy".to_owned(), json!(policy));
        changed.push("signup_policy");
    }
    if body.review_production.is_some() || body.review_preview.is_some() {
        let current = settings::get_setting(&transaction, REVIEW_POLICY_KEY)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?
            .map(|setting| setting.value)
            .unwrap_or_else(|| json!({}));
        let current_production = current
            .get("production")
            .and_then(|v| v.as_str())
            .unwrap_or("manual");
        let current_preview = current
            .get("preview")
            .and_then(|v| v.as_str())
            .unwrap_or("auto");
        let production = body
            .review_production
            .as_deref()
            .unwrap_or(current_production);
        let preview = body.review_preview.as_deref().unwrap_or(current_preview);
        before.insert(
            "review_policy".to_owned(),
            json!({ "production": current_production, "preview": current_preview }),
        );
        settings::set_setting(
            &transaction,
            REVIEW_POLICY_KEY,
            SystemSettingValueKind::Json,
            json!({ "production": production, "preview": preview }),
        )
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        after.insert(
            "review_policy".to_owned(),
            json!({ "production": production, "preview": preview }),
        );
        changed.push("review_policy");
    }

    if let Some(root) = body.storage_root.as_deref() {
        let configured_root = state.config.read().unwrap().storage.root.clone();
        before.insert(
            "storage_root".to_owned(),
            json!(
                setting_string(&transaction, STORAGE_ROOT_KEY, OP)
                    .await?
                    .unwrap_or(configured_root)
            ),
        );
        settings::set_string(&transaction, STORAGE_ROOT_KEY, root)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        nodes::update_work_roots(&transaction, &format!("{root}/node"))
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        after.insert("storage_root".to_owned(), json!(root));
        changed.push("storage_root");
    }

    create_settings_audit_in_transaction(&transaction, data.user_id, changed, before, after)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    if let Some(config_update) = storage_config_update.as_ref() {
        config_update
            .persisted
            .save(state.config_path())
            .map_err(|error| AppError::Infrastructure {
                op: OP,
                source: anyhow::anyhow!(error),
            })?;
    }
    if let Err(source) = transaction.commit().await {
        if let Some(config_update) = storage_config_update.as_ref()
            && let Err(restore_error) = config_update.original.save(state.config_path())
        {
            tracing::error!(
                operation = "admin.settings.restore_config",
                %restore_error,
                "failed to restore bootstrap config after transaction failure"
            );
        }
        return Err(AppError::Infrastructure {
            op: OP,
            source: source.into(),
        });
    }

    if let Some(config_update) = storage_config_update {
        *state.config.write().unwrap() = config_update.runtime;
        if let Some(root) = body.storage_root.as_deref()
            && let Err(error) = crate::infra::node_manager::config_file::update_storage_root(
                &config_update.local_node_config,
                root,
            )
        {
            tracing::warn!(
                operation = "admin.settings.update_local_node_config",
                %error,
                "failed to update generated local node config"
            );
        }
    }

    get(State(state)).await
}

#[cfg(test)]
mod tests {
    use axum::{Json, extract::State};
    use sea_orm::{DatabaseConnection, MockDatabase, TransactionTrait};
    use time::OffsetDateTime;
    use uuid::Uuid;

    use crate::{
        infra::{config::ControlApiConfig, http::extractors::Session},
        state::ControlApiState,
    };

    use super::{
        UpdateSettingsRequest, create_settings_audit_in_transaction, prepare_settings_update,
        update,
    };

    fn session() -> Session {
        Session {
            data: grass_session::SessionData {
                user_id: Uuid::now_v7(),
                created_at: OffsetDateTime::UNIX_EPOCH,
                last_accessed_at: OffsetDateTime::UNIX_EPOCH,
            },
            session_id: "test-session".to_owned(),
        }
    }

    fn setting_model(
        key: &str,
        value: serde_json::Value,
    ) -> crate::infra::database::entity::system_setting::Model {
        crate::infra::database::entity::system_setting::Model {
            id: Uuid::now_v7(),
            key: key.to_owned(),
            value_kind: crate::infra::database::entity::SystemSettingValueKind::String,
            value,
            is_secret: false,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    fn site_name_update_database(audit_succeeds: bool) -> DatabaseConnection {
        let old = setting_model(super::SITE_NAME_KEY, serde_json::json!("Old Name"));
        let updated = setting_model(super::SITE_NAME_KEY, serde_json::json!("New Name"));
        let mock = MockDatabase::new(sea_orm::DbBackend::Postgres).append_query_results([
            vec![old.clone()],
            vec![old],
            vec![updated.clone()],
            Vec::<crate::infra::database::entity::system_setting::Model>::new(),
            vec![updated],
            Vec::<crate::infra::database::entity::system_setting::Model>::new(),
            Vec::<crate::infra::database::entity::system_setting::Model>::new(),
            Vec::<crate::infra::database::entity::system_setting::Model>::new(),
            Vec::<crate::infra::database::entity::system_setting::Model>::new(),
        ]);
        if audit_succeeds {
            mock.append_exec_results([sea_orm::MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection()
        } else {
            mock.into_connection()
        }
    }

    fn site_name_update_request() -> UpdateSettingsRequest {
        UpdateSettingsRequest {
            site_name: Some(" New Name ".to_owned()),
            site_url: None,
            public_base_url: None,
            storage_root: None,
            signup_policy: None,
            review_production: None,
            review_preview: None,
        }
    }

    fn invalid_later_field_request() -> UpdateSettingsRequest {
        UpdateSettingsRequest {
            site_name: Some("Valid Name".to_owned()),
            site_url: None,
            public_base_url: None,
            storage_root: None,
            signup_policy: Some("invalid".to_owned()),
            review_production: None,
            review_preview: None,
        }
    }

    #[tokio::test]
    async fn invalid_later_field_does_not_execute_an_earlier_valid_update() {
        assert!(
            prepare_settings_update(invalid_later_field_request(), "test.settings.prepare")
                .is_err()
        );
        let now = OffsetDateTime::UNIX_EPOCH;
        let inserted_site_name = crate::infra::database::entity::system_setting::Model {
            id: Uuid::now_v7(),
            key: super::SITE_NAME_KEY.to_owned(),
            value_kind: crate::infra::database::entity::SystemSettingValueKind::String,
            value: serde_json::json!("Valid Name"),
            is_secret: false,
            created_at: now,
            updated_at: now,
        };
        let db = MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([
                Vec::<crate::infra::database::entity::system_setting::Model>::new(),
                vec![inserted_site_name],
            ])
            .into_connection();
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        assert!(state.database.set(db.clone()).is_ok());

        let result = update(State(state), session(), Json(invalid_later_field_request())).await;

        assert!(result.is_err());
        let statements = format!("{:?}", db.into_transaction_log());
        assert!(!statements.contains("INSERT"), "{statements}");
        assert!(!statements.contains("UPDATE"), "{statements}");
    }

    #[tokio::test]
    async fn setting_update_and_domain_audit_share_one_transaction() {
        let db = site_name_update_database(true);
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        assert!(state.database.set(db.clone()).is_ok());

        let result = update(State(state), session(), Json(site_name_update_request())).await;

        assert!(result.is_ok());
        let transaction_log = db.into_transaction_log();
        assert!(
            transaction_log.iter().any(|transaction| {
                let statements = format!("{transaction:?}");
                statements.contains("UPDATE \\\"system_settings\\\"")
                    && statements.contains("INSERT INTO \\\"audit_events\\\"")
            }),
            "{transaction_log:?}"
        );
    }

    #[tokio::test]
    async fn settings_audit_is_written_through_the_caller_transaction() {
        let db = MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_exec_results([sea_orm::MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let transaction = db.begin().await.expect("begin transaction");

        create_settings_audit_in_transaction(
            &transaction,
            Uuid::now_v7(),
            vec!["site_name"],
            serde_json::Map::new(),
            serde_json::Map::new(),
        )
        .await
        .expect("create settings audit");
        transaction.commit().await.expect("commit transaction");

        let transaction_log = db.into_transaction_log();
        assert_eq!(transaction_log.len(), 1, "{transaction_log:?}");
        let statements = format!("{:?}", transaction_log[0]);
        assert!(
            statements.contains("INSERT INTO \\\"audit_events\\\""),
            "{statements}"
        );
        assert!(statements.contains("COMMIT"), "{statements}");
    }

    #[tokio::test]
    async fn audit_failure_fails_the_settings_update() {
        let db = site_name_update_database(false);
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        assert!(state.database.set(db).is_ok());

        let result = update(State(state), session(), Json(site_name_update_request())).await;

        assert!(result.is_err());
    }
}
