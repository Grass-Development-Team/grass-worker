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
    let _setup_guard = state.lock_setup().await;
    let db = super::setup_database(&state, "setup.finish.database")?;
    super::ensure_setup_mutation_allowed(db, "setup.finish.ready_mode").await?;

    let admin_created =
        users::any_user_exists(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: "setup.finish.check_admin",
                source,
            })?;
    let node_created =
        nodes::any_node_exists(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: "setup.finish.check_node",
                source,
            })?;
    let site_configured = settings::get_setting(db, "site.name")
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.finish.check_site",
            source,
        })?
        .is_some();
    let storage_configured = settings::get_setting(db, "storage.root")
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.finish.check_storage",
            source,
        })?
        .is_some();

    if !setup_prerequisites_complete(
        admin_created,
        node_created,
        site_configured,
        storage_configured,
    ) {
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

    // Ready mode just unlocked the internal Node API — start the managed
    // local node now if setup generated its config.
    let mut local_node_started = false;
    let (auto_start, config_path) = {
        let config = state.config.read().unwrap();
        (
            config.node_manager.auto_start_local_node,
            config.node_manager.local_node_config.clone(),
        )
    };
    if auto_start && crate::infra::node_manager::config_file::exists(&config_path) {
        match state.node_manager.start().await {
            Ok(_) => local_node_started = true,
            Err(error) => tracing::warn!(
                operation = "setup.finish.local_node_start",
                %error,
                "failed to start managed local node"
            ),
        }
    }

    Ok(ok_response(json!({
        "setup_finished": true,
        "local_node_started": local_node_started,
    })))
}

fn setup_prerequisites_complete(admin: bool, node: bool, site: bool, storage: bool) -> bool {
    admin && node && site && storage
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_setup_prerequisites_include_storage() {
        assert!(setup_prerequisites_complete(true, true, true, true));
        assert!(!setup_prerequisites_complete(true, true, true, false));
    }
}
