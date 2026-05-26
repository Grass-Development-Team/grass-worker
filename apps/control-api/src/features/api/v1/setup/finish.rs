use axum::{extract::State, response::IntoResponse};
use serde_json::json;

use crate::{
    domain::{nodes, settings, users},
    infra::{
        database::entity::SystemSettingValueKind,
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub async fn handler(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    let db = super::setup_database(&state, "setup.finish.database")?;
    super::ensure_setup_mutation_allowed(db, "setup.finish.ready_mode").await?;

    let admin_created = users::any_user_exists(db).await.unwrap_or(false);
    let node_created = nodes::any_node_exists(db).await.unwrap_or(false);
    let site_configured = settings::get_setting(db, "site.name")
        .await
        .map(|setting| setting.is_some())
        .unwrap_or(false);

    if !(admin_created && node_created && site_configured) {
        return Err(AppError::Validation {
            op: "setup.finish.incomplete",
            message: "all setup steps must be completed before finish".to_owned(),
        });
    }

    settings::set_setting(
        db,
        "setup.finished",
        SystemSettingValueKind::Boolean,
        json!(true),
    )
    .await
    .map_err(|source| AppError::Infrastructure {
        op: "setup.finish.save",
        source,
    })?;

    Ok(ok_response(json!({ "setup_finished": true })))
}
