use axum::{Json, extract::State, response::IntoResponse};
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

async fn setting_string(
    db: &sea_orm::DatabaseConnection,
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

fn validate_review_mode(value: &str, op: &'static str) -> Result<(), AppError> {
    match value {
        "auto" | "manual" => Ok(()),
        _ => Err(AppError::Validation {
            op,
            message: format!("review mode must be auto or manual, got {value}"),
        }),
    }
}

/// PATCH /api/v1/admin/settings
pub async fn update(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Json(body): Json<UpdateSettingsRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.settings.update";
    let db = super::database(&state, OP)?;

    let mut changed: Vec<&'static str> = Vec::new();

    if let Some(name) = body.site_name.as_deref().map(str::trim) {
        if name.is_empty() {
            return Err(AppError::Validation {
                op: OP,
                message: "site name cannot be empty".to_owned(),
            });
        }
        settings::set_string(db, SITE_NAME_KEY, name)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        changed.push("site_name");
    }
    if let Some(url) = body.site_url.as_deref() {
        let url = crate::features::api::v1::setup::site::validate_site_url(url, OP)?;
        settings::set_string(db, SITE_URL_KEY, &url)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        changed.push("site_url");
    }
    if let Some(url) = body.public_base_url.as_deref() {
        let url = crate::features::api::v1::setup::site::validate_site_url(url, OP)?;
        settings::set_string(db, PUBLIC_BASE_URL_KEY, &url)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        changed.push("public_base_url");
    }
    if let Some(policy) = body.signup_policy.as_deref() {
        if !matches!(policy, "open" | "invite_only" | "closed") {
            return Err(AppError::Validation {
                op: OP,
                message: format!("signup policy must be open, invite_only or closed, got {policy}"),
            });
        }
        settings::set_string(db, SIGNUP_POLICY_KEY, policy)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        changed.push("signup_policy");
    }
    if body.review_production.is_some() || body.review_preview.is_some() {
        let current = settings::get_setting(db, REVIEW_POLICY_KEY)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?
            .map(|setting| setting.value)
            .unwrap_or_else(|| json!({}));
        let production = body.review_production.as_deref().unwrap_or_else(|| {
            current
                .get("production")
                .and_then(|v| v.as_str())
                .unwrap_or("manual")
        });
        let preview = body.review_preview.as_deref().unwrap_or_else(|| {
            current
                .get("preview")
                .and_then(|v| v.as_str())
                .unwrap_or("auto")
        });
        validate_review_mode(production, OP)?;
        validate_review_mode(preview, OP)?;
        settings::set_setting(
            db,
            REVIEW_POLICY_KEY,
            SystemSettingValueKind::Json,
            json!({ "production": production, "preview": preview }),
        )
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        changed.push("review_policy");
    }

    if let Some(root) = body.storage_root.as_deref() {
        let root = root.trim().trim_end_matches('/');
        if root.is_empty() || !std::path::Path::new(root).is_absolute() {
            return Err(AppError::Validation {
                op: OP,
                message: "storage root must be a non-empty absolute path".to_owned(),
            });
        }
        settings::set_string(db, STORAGE_ROOT_KEY, root)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        nodes::update_work_roots(db, &format!("{root}/node"))
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;

        // Keep the bootstrap config and any generated local node config in
        // sync, mirroring the setup storage step.
        let mut persisted = crate::infra::config::ControlApiConfig::load_persisted(
            state.config_path(),
        )
        .map_err(|error| AppError::Infrastructure {
            op: OP,
            source: anyhow::anyhow!(error),
        })?;
        persisted.ensure_secret_key();
        persisted.storage.root = root.to_owned();
        persisted
            .save(state.config_path())
            .map_err(|error| AppError::Infrastructure {
                op: OP,
                source: anyhow::anyhow!(error),
            })?;
        state.config.write().unwrap().storage.root = root.to_owned();

        let local_node_config = state
            .config
            .read()
            .unwrap()
            .node_manager
            .local_node_config
            .clone();
        if let Err(error) =
            crate::infra::node_manager::config_file::update_storage_root(&local_node_config, root)
        {
            tracing::warn!(
                operation = "admin.settings.update_local_node_config",
                %error,
                "failed to update generated local node config"
            );
        }
        changed.push("storage_root");
    }

    if changed.is_empty() {
        return Err(AppError::Validation {
            op: OP,
            message: "nothing to update".to_owned(),
        });
    }

    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            team_id: None,
            action: "settings.updated".to_owned(),
            target_type: "settings".to_owned(),
            target_id: None,
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "changed": changed }),
        },
    )
    .await;

    get(State(state)).await
}
