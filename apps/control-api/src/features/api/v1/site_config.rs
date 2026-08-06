use axum::{extract::State, response::IntoResponse};
use serde::Serialize;

use crate::{
    domain::settings,
    infra::error::{AppError, ok_response},
    state::ControlApiState,
};

const DEFAULT_SITE_NAME: &str = "Grass Worker";

#[derive(Serialize)]
struct SiteConfigResponse {
    site_name: String,
    logo_url: Option<String>,
    version: &'static str,
}

pub async fn get(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    let (site_name, logo_url) = if let Some(database) = state.try_database() {
        let site_name = settings::get_setting(database, "site.name")
            .await
            .map_err(|source| AppError::Infrastructure {
                op: "site_config.get",
                source,
            })?
            .and_then(|setting| setting.value.as_str().map(str::to_owned))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_SITE_NAME.to_owned());
        let logo_url = settings::get_setting(database, "site.logo_url")
            .await
            .map_err(|source| AppError::Infrastructure {
                op: "site_config.get_logo",
                source,
            })?
            .and_then(|setting| setting.value.as_str().map(str::to_owned))
            .filter(|value| !value.trim().is_empty());
        (site_name, logo_url)
    } else {
        (DEFAULT_SITE_NAME.to_owned(), None)
    };

    Ok(ok_response(SiteConfigResponse {
        site_name,
        logo_url,
        version: env!("CARGO_PKG_VERSION"),
    }))
}
