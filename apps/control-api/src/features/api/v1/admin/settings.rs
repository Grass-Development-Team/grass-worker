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
    let config = state.config.read().unwrap().clone();
    let storage_root = setting_string(db, STORAGE_ROOT_KEY, OP)
        .await?
        .unwrap_or_else(|| config.storage.root.clone());
    let secret_key = config.secrets.secret_key.trim();

    Ok(ok_response(json!({
        "site": {
            "name": setting_string(db, SITE_NAME_KEY, OP).await?,
            "url": setting_string(db, SITE_URL_KEY, OP).await?,
            "public_base_url": setting_string(db, PUBLIC_BASE_URL_KEY, OP).await?,
        },
        "storage": {
            "root": storage_root,
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
        "server": {
            "host": config.server.host,
            "port": config.server.port,
        },
        "database": {
            "url_configured": !config.database.url.trim().is_empty(),
        },
        "redis": {
            "backend": config.redis.backend,
            "url_configured": !config.redis.url.trim().is_empty(),
        },
        "secrets": {
            "secret_key_configured": !secret_key.is_empty()
                && secret_key != "change-me"
                && secret_key.len() >= 32,
            "git_credentials_configured": config.secrets.git_credentials.active_key().is_ok(),
        },
        "session": config.session,
        "audit": config.audit,
        "node_manager": config.node_manager,
        "migration": config.migration,
        "log": config.log,
        "restart_required_sections": ["server", "redis", "node_manager", "migration", "log"],
    })))
}

#[derive(Default, Deserialize)]
pub struct UpdateSettingsRequest {
    pub site_name: Option<String>,
    pub site_url: Option<String>,
    pub public_base_url: Option<String>,
    pub storage_root: Option<String>,
    pub signup_policy: Option<String>,
    pub review_production: Option<String>,
    pub review_preview: Option<String>,
    pub server_host: Option<String>,
    pub server_port: Option<u16>,
    pub redis_backend: Option<String>,
    pub session_cookie_secure: Option<bool>,
    pub session_idle_ttl_seconds: Option<u64>,
    pub session_ttl_seconds: Option<u64>,
    pub audit_retention_days: Option<u64>,
    pub node_manager_auto_start_local_node: Option<bool>,
    pub node_manager_local_node_binary: Option<String>,
    pub node_manager_local_node_config: Option<String>,
    pub node_manager_restart_on_exit: Option<bool>,
    pub migration_auto_migrate: Option<bool>,
    pub log_level: Option<String>,
    pub log_format: Option<String>,
}

struct PreparedSettingsUpdate {
    site_name: Option<String>,
    site_url: Option<String>,
    public_base_url: Option<String>,
    storage_root: Option<String>,
    signup_policy: Option<String>,
    review_production: Option<String>,
    review_preview: Option<String>,
    server_host: Option<std::net::IpAddr>,
    server_port: Option<u16>,
    redis_backend: Option<grass_cache::CacheBackend>,
    session_cookie_secure: Option<bool>,
    session_idle_ttl_seconds: Option<u64>,
    session_ttl_seconds: Option<u64>,
    audit_retention_days: Option<u64>,
    node_manager_auto_start_local_node: Option<bool>,
    node_manager_local_node_binary: Option<String>,
    node_manager_local_node_config: Option<String>,
    node_manager_restart_on_exit: Option<bool>,
    migration_auto_migrate: Option<bool>,
    log_level: Option<String>,
    log_format: Option<crate::infra::config::log::LogFormat>,
}

struct PreparedConfigUpdate {
    original: crate::infra::config::ControlApiConfig,
    persisted: crate::infra::config::ControlApiConfig,
    runtime: crate::infra::config::ControlApiConfig,
    local_node_config: String,
}

impl PreparedSettingsUpdate {
    fn has_config_update(&self) -> bool {
        self.storage_root.is_some()
            || self.server_host.is_some()
            || self.server_port.is_some()
            || self.redis_backend.is_some()
            || self.session_cookie_secure.is_some()
            || self.session_idle_ttl_seconds.is_some()
            || self.session_ttl_seconds.is_some()
            || self.audit_retention_days.is_some()
            || self.node_manager_auto_start_local_node.is_some()
            || self.node_manager_local_node_binary.is_some()
            || self.node_manager_local_node_config.is_some()
            || self.node_manager_restart_on_exit.is_some()
            || self.migration_auto_migrate.is_some()
            || self.log_level.is_some()
            || self.log_format.is_some()
    }

    fn apply_to_config(
        &self,
        config: &mut crate::infra::config::ControlApiConfig,
        op: &'static str,
    ) -> Result<(), AppError> {
        if let Some(root) = self.storage_root.as_deref() {
            config.storage.root = root.to_owned();
        }
        if let Some(host) = self.server_host {
            config.server.host = host;
        }
        if let Some(port) = self.server_port {
            config.server.port = port;
        }
        if let Some(backend) = self.redis_backend {
            config.redis.backend = backend;
        }
        if let Some(cookie_secure) = self.session_cookie_secure {
            config.session.cookie_secure = cookie_secure;
        }
        if let Some(seconds) = self.session_idle_ttl_seconds {
            config.session.idle_ttl_seconds = seconds;
        }
        if let Some(seconds) = self.session_ttl_seconds {
            config.session.session_ttl_seconds = seconds;
        }
        if config.session.idle_ttl_seconds > config.session.session_ttl_seconds {
            return Err(AppError::Validation {
                op,
                message: "session idle TTL cannot exceed the absolute session TTL".to_owned(),
            });
        }
        if let Some(days) = self.audit_retention_days {
            config.audit.retention_days = days;
        }
        if let Some(auto_start) = self.node_manager_auto_start_local_node {
            config.node_manager.auto_start_local_node = auto_start;
        }
        if let Some(binary) = self.node_manager_local_node_binary.as_deref() {
            config.node_manager.local_node_binary = binary.to_owned();
        }
        if let Some(path) = self.node_manager_local_node_config.as_deref() {
            config.node_manager.local_node_config = path.to_owned();
        }
        if let Some(restart) = self.node_manager_restart_on_exit {
            config.node_manager.restart_on_exit = restart;
        }
        if let Some(auto_migrate) = self.migration_auto_migrate {
            config.migration.auto_migrate = auto_migrate;
        }
        if let Some(level) = self.log_level.as_deref() {
            config.log.level = level.to_owned();
        }
        if let Some(format) = self.log_format.clone() {
            config.log.format = format;
        }
        Ok(())
    }

    fn append_config_audit(
        &self,
        original: &crate::infra::config::ControlApiConfig,
        updated: &crate::infra::config::ControlApiConfig,
        changed: &mut Vec<&'static str>,
        before: &mut serde_json::Map<String, serde_json::Value>,
        after: &mut serde_json::Map<String, serde_json::Value>,
    ) {
        macro_rules! record {
            ($present:expr, $name:literal, $old:expr, $new:expr) => {
                if $present {
                    changed.push($name);
                    before.insert($name.to_owned(), json!($old));
                    after.insert($name.to_owned(), json!($new));
                }
            };
        }

        record!(
            self.server_host.is_some(),
            "server_host",
            original.server.host,
            updated.server.host
        );
        record!(
            self.server_port.is_some(),
            "server_port",
            original.server.port,
            updated.server.port
        );
        record!(
            self.redis_backend.is_some(),
            "redis_backend",
            original.redis.backend,
            updated.redis.backend
        );
        record!(
            self.session_cookie_secure.is_some(),
            "session_cookie_secure",
            original.session.cookie_secure,
            updated.session.cookie_secure
        );
        record!(
            self.session_idle_ttl_seconds.is_some(),
            "session_idle_ttl_seconds",
            original.session.idle_ttl_seconds,
            updated.session.idle_ttl_seconds
        );
        record!(
            self.session_ttl_seconds.is_some(),
            "session_ttl_seconds",
            original.session.session_ttl_seconds,
            updated.session.session_ttl_seconds
        );
        record!(
            self.audit_retention_days.is_some(),
            "audit_retention_days",
            original.audit.retention_days,
            updated.audit.retention_days
        );
        record!(
            self.node_manager_auto_start_local_node.is_some(),
            "node_manager_auto_start_local_node",
            original.node_manager.auto_start_local_node,
            updated.node_manager.auto_start_local_node
        );
        record!(
            self.node_manager_local_node_binary.is_some(),
            "node_manager_local_node_binary",
            original.node_manager.local_node_binary,
            updated.node_manager.local_node_binary
        );
        record!(
            self.node_manager_local_node_config.is_some(),
            "node_manager_local_node_config",
            original.node_manager.local_node_config,
            updated.node_manager.local_node_config
        );
        record!(
            self.node_manager_restart_on_exit.is_some(),
            "node_manager_restart_on_exit",
            original.node_manager.restart_on_exit,
            updated.node_manager.restart_on_exit
        );
        record!(
            self.migration_auto_migrate.is_some(),
            "migration_auto_migrate",
            original.migration.auto_migrate,
            updated.migration.auto_migrate
        );
        record!(
            self.log_level.is_some(),
            "log_level",
            original.log.level,
            updated.log.level
        );
        record!(
            self.log_format.is_some(),
            "log_format",
            original.log.format,
            updated.log.format
        );
    }
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

    let server_host = body
        .server_host
        .as_deref()
        .map(|value| {
            value
                .trim()
                .parse::<std::net::IpAddr>()
                .map_err(|_| AppError::Validation {
                    op,
                    message: "server host must be an IPv4 or IPv6 address".to_owned(),
                })
        })
        .transpose()?;
    if body.server_port == Some(0) {
        return Err(AppError::Validation {
            op,
            message: "server port must be greater than zero".to_owned(),
        });
    }

    let redis_backend = body
        .redis_backend
        .as_deref()
        .map(|value| match value.trim() {
            "moka" => Ok(grass_cache::CacheBackend::Moka),
            "redis" => Ok(grass_cache::CacheBackend::Redis),
            value => Err(AppError::Validation {
                op,
                message: format!("redis backend must be moka or redis, got {value}"),
            }),
        })
        .transpose()?;

    if body.session_idle_ttl_seconds == Some(0) || body.session_ttl_seconds == Some(0) {
        return Err(AppError::Validation {
            op,
            message: "session TTL values must be greater than zero".to_owned(),
        });
    }
    if body
        .session_idle_ttl_seconds
        .is_some_and(|seconds| seconds > i64::MAX as u64)
        || body
            .session_ttl_seconds
            .is_some_and(|seconds| seconds > i64::MAX as u64)
    {
        return Err(AppError::Validation {
            op,
            message: "session TTL values are too large".to_owned(),
        });
    }
    if let (Some(idle), Some(absolute)) = (body.session_idle_ttl_seconds, body.session_ttl_seconds)
        && idle > absolute
    {
        return Err(AppError::Validation {
            op,
            message: "session idle TTL cannot exceed the absolute session TTL".to_owned(),
        });
    }

    let node_manager_local_node_binary = body
        .node_manager_local_node_binary
        .map(|value| value.trim().to_owned());
    if node_manager_local_node_binary
        .as_deref()
        .is_some_and(str::is_empty)
    {
        return Err(AppError::Validation {
            op,
            message: "local node binary cannot be empty".to_owned(),
        });
    }
    let node_manager_local_node_config = body
        .node_manager_local_node_config
        .map(|value| value.trim().to_owned());
    if node_manager_local_node_config
        .as_deref()
        .is_some_and(str::is_empty)
    {
        return Err(AppError::Validation {
            op,
            message: "local node config path cannot be empty".to_owned(),
        });
    }

    let log_level = body.log_level.map(|value| value.trim().to_owned());
    if let Some(level) = log_level.as_deref()
        && tracing_subscriber::EnvFilter::try_new(level).is_err()
    {
        return Err(AppError::Validation {
            op,
            message: "log level must be a valid tracing filter".to_owned(),
        });
    }
    let log_format = body
        .log_format
        .as_deref()
        .map(|value| match value.trim() {
            "pretty" => Ok(crate::infra::config::log::LogFormat::Pretty),
            "json" => Ok(crate::infra::config::log::LogFormat::Json),
            value => Err(AppError::Validation {
                op,
                message: format!("log format must be pretty or json, got {value}"),
            }),
        })
        .transpose()?;

    let prepared = PreparedSettingsUpdate {
        site_name,
        site_url,
        public_base_url,
        storage_root,
        signup_policy,
        review_production,
        review_preview,
        server_host,
        server_port: body.server_port,
        redis_backend,
        session_cookie_secure: body.session_cookie_secure,
        session_idle_ttl_seconds: body.session_idle_ttl_seconds,
        session_ttl_seconds: body.session_ttl_seconds,
        audit_retention_days: body.audit_retention_days,
        node_manager_auto_start_local_node: body.node_manager_auto_start_local_node,
        node_manager_local_node_binary,
        node_manager_local_node_config,
        node_manager_restart_on_exit: body.node_manager_restart_on_exit,
        migration_auto_migrate: body.migration_auto_migrate,
        log_level,
        log_format,
    };
    if prepared.site_name.is_none()
        && prepared.site_url.is_none()
        && prepared.public_base_url.is_none()
        && prepared.storage_root.is_none()
        && prepared.signup_policy.is_none()
        && prepared.review_production.is_none()
        && prepared.review_preview.is_none()
        && prepared.server_host.is_none()
        && prepared.server_port.is_none()
        && prepared.redis_backend.is_none()
        && prepared.session_cookie_secure.is_none()
        && prepared.session_idle_ttl_seconds.is_none()
        && prepared.session_ttl_seconds.is_none()
        && prepared.audit_retention_days.is_none()
        && prepared.node_manager_auto_start_local_node.is_none()
        && prepared.node_manager_local_node_binary.is_none()
        && prepared.node_manager_local_node_config.is_none()
        && prepared.node_manager_restart_on_exit.is_none()
        && prepared.migration_auto_migrate.is_none()
        && prepared.log_level.is_none()
        && prepared.log_format.is_none()
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
    let config_update = if body.has_config_update() {
        let original = crate::infra::config::ControlApiConfig::load_persisted(state.config_path())
            .map_err(|error| AppError::Infrastructure {
                op: OP,
                source: anyhow::anyhow!(error),
            })?;
        let mut persisted = original.clone();
        persisted.ensure_secret_key();
        body.apply_to_config(&mut persisted, OP)?;
        let mut runtime = state.config.read().unwrap().clone();
        body.apply_to_config(&mut runtime, OP)?;
        let local_node_config = runtime.node_manager.local_node_config.clone();
        Some(PreparedConfigUpdate {
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

    if let Some(config_update) = config_update.as_ref() {
        body.append_config_audit(
            &config_update.original,
            &config_update.persisted,
            &mut changed,
            &mut before,
            &mut after,
        );
    }

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

    if let Some(config_update) = config_update.as_ref() {
        config_update
            .persisted
            .save(state.config_path())
            .map_err(|error| AppError::Infrastructure {
                op: OP,
                source: anyhow::anyhow!(error),
            })?;
    }
    if let Err(source) = transaction.commit().await {
        if let Some(config_update) = config_update.as_ref()
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

    if let Some(config_update) = config_update {
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
    use std::{fs, time::SystemTime};

    use axum::{Json, body::to_bytes, extract::State, response::IntoResponse};
    use sea_orm::{DatabaseConnection, MockDatabase, TransactionTrait};
    use time::OffsetDateTime;
    use uuid::Uuid;

    use crate::{
        infra::{config::ControlApiConfig, http::extractors::Session},
        state::ControlApiState,
    };

    use super::{
        UpdateSettingsRequest, create_settings_audit_in_transaction, get, prepare_settings_update,
        update,
    };

    #[tokio::test]
    async fn settings_response_lists_non_secret_control_api_config_without_leaking_secrets() {
        let db = MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([
                Vec::<crate::infra::database::entity::system_setting::Model>::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ])
            .into_connection();
        let mut config = ControlApiConfig::default();
        config.server.host = "0.0.0.0".parse().unwrap();
        config.server.port = 9000;
        config.database.url = "postgres://user:database-secret@db.example/grass".to_owned();
        config.redis.backend = grass_cache::CacheBackend::Moka;
        config.redis.url = "redis://:redis-secret@cache.example/0".to_owned();
        config.storage.root = "/srv/grass".to_owned();
        config.secrets.secret_key = "control-api-secret-value-that-must-not-leak".to_owned();
        config.session.cookie_secure = false;
        config.session.idle_ttl_seconds = 1_200;
        config.session.session_ttl_seconds = 86_400;
        config.audit.retention_days = 120;
        config.node_manager.auto_start_local_node = true;
        config.node_manager.local_node_binary = "/usr/local/bin/grass-node".to_owned();
        config.node_manager.local_node_config = "/etc/grass/node.toml".to_owned();
        config.node_manager.restart_on_exit = false;
        config.migration.auto_migrate = true;
        config.log.level = "warn".to_owned();
        config.log.format = crate::infra::config::log::LogFormat::Json;
        let state = ControlApiState::new(config, "unused.toml");
        assert!(state.database.set(db).is_ok());

        let response = get(State(state)).await.unwrap().into_response();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let data = &body["data"];

        assert_eq!(
            data["server"],
            serde_json::json!({ "host": "0.0.0.0", "port": 9000 })
        );
        assert_eq!(
            data["database"],
            serde_json::json!({ "url_configured": true })
        );
        assert_eq!(
            data["redis"],
            serde_json::json!({ "backend": "moka", "url_configured": true })
        );
        assert_eq!(
            data["session"],
            serde_json::json!({
                "cookie_secure": false,
                "idle_ttl_seconds": 1_200,
                "session_ttl_seconds": 86_400,
            })
        );
        assert_eq!(data["audit"], serde_json::json!({ "retention_days": 120 }));
        assert_eq!(
            data["node_manager"],
            serde_json::json!({
                "auto_start_local_node": true,
                "local_node_binary": "/usr/local/bin/grass-node",
                "local_node_config": "/etc/grass/node.toml",
                "restart_on_exit": false,
            })
        );
        assert_eq!(
            data["migration"],
            serde_json::json!({ "auto_migrate": true })
        );
        assert_eq!(
            data["log"],
            serde_json::json!({ "level": "warn", "format": "json" })
        );
        assert_eq!(
            data["secrets"],
            serde_json::json!({
                "secret_key_configured": true,
                "git_credentials_configured": false,
            })
        );
        assert_eq!(
            data["restart_required_sections"],
            serde_json::json!(["server", "redis", "node_manager", "migration", "log"])
        );

        let serialized = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(!serialized.contains("database-secret"));
        assert!(!serialized.contains("redis-secret"));
        assert!(!serialized.contains("control-api-secret-value"));
    }

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
            ..UpdateSettingsRequest::default()
        }
    }

    fn invalid_later_field_request() -> UpdateSettingsRequest {
        UpdateSettingsRequest {
            site_name: Some("Valid Name".to_owned()),
            signup_policy: Some("invalid".to_owned()),
            ..UpdateSettingsRequest::default()
        }
    }

    fn control_api_config_update_request() -> UpdateSettingsRequest {
        serde_json::from_value(serde_json::json!({
            "server_host": "0.0.0.0",
            "server_port": 9000,
            "redis_backend": "moka",
            "session_cookie_secure": false,
            "session_idle_ttl_seconds": 1_200,
            "session_ttl_seconds": 86_400,
            "audit_retention_days": 120,
            "node_manager_auto_start_local_node": true,
            "node_manager_local_node_binary": "/usr/local/bin/grass-node",
            "node_manager_local_node_config": "/etc/grass/node.toml",
            "node_manager_restart_on_exit": false,
            "migration_auto_migrate": true,
            "log_level": "warn,grass_control_api=debug",
            "log_format": "json"
        }))
        .unwrap()
    }

    #[test]
    fn control_api_config_only_update_is_accepted() {
        assert!(
            prepare_settings_update(control_api_config_update_request(), "test.settings.prepare")
                .is_ok()
        );
    }

    #[test]
    fn session_ttl_must_fit_timestamp_arithmetic() {
        let request: UpdateSettingsRequest = serde_json::from_value(serde_json::json!({
            "session_ttl_seconds": (i64::MAX as u64) + 1
        }))
        .unwrap();

        assert!(prepare_settings_update(request, "test.settings.prepare").is_err());
    }

    #[tokio::test]
    async fn control_api_config_update_is_persisted_and_applied_to_runtime_state() {
        let db = MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([
                Vec::<crate::infra::database::entity::system_setting::Model>::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ])
            .append_exec_results([sea_orm::MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("grass-worker-admin-settings-{unique}.toml"));
        let initial = ControlApiConfig::default();
        initial.save(&path).unwrap();
        let state = ControlApiState::new(initial, path.to_string_lossy());
        assert!(state.database.set(db).is_ok());

        let result = update(
            State(state.clone()),
            session(),
            Json(control_api_config_update_request()),
        )
        .await;

        assert!(result.is_ok());
        let runtime = state.config.read().unwrap().clone();
        let persisted = ControlApiConfig::load_persisted(&path).unwrap();
        fs::remove_file(path).unwrap();
        for config in [&runtime, &persisted] {
            assert_eq!(config.server.host.to_string(), "0.0.0.0");
            assert_eq!(config.server.port, 9000);
            assert_eq!(config.redis.backend, grass_cache::CacheBackend::Moka);
            assert!(!config.session.cookie_secure);
            assert_eq!(config.session.idle_ttl_seconds, 1_200);
            assert_eq!(config.session.session_ttl_seconds, 86_400);
            assert_eq!(config.audit.retention_days, 120);
            assert!(config.node_manager.auto_start_local_node);
            assert_eq!(
                config.node_manager.local_node_binary,
                "/usr/local/bin/grass-node"
            );
            assert_eq!(
                config.node_manager.local_node_config,
                "/etc/grass/node.toml"
            );
            assert!(!config.node_manager.restart_on_exit);
            assert!(config.migration.auto_migrate);
            assert_eq!(config.log.level, "warn,grass_control_api=debug");
            assert_eq!(
                config.log.format,
                crate::infra::config::log::LogFormat::Json
            );
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
