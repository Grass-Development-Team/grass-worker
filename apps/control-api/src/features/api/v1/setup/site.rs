use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::TransactionTrait;
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::settings,
    infra::error::{AppError, ok_response},
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct SiteSetupRequest {
    pub name: String,
    pub site_url: String,
    pub public_base_url: String,
}

pub async fn handler(
    State(state): State<ControlApiState>,
    Json(body): Json<SiteSetupRequest>,
) -> Result<impl IntoResponse, AppError> {
    let _setup_guard = state.lock_setup().await;
    let db = super::setup_database(&state, "setup.site.database")?;
    super::ensure_setup_mutation_allowed(db, "setup.site.ready_mode").await?;

    let name = body.name.trim();
    if name.is_empty() {
        return Err(AppError::Validation {
            op: "setup.site.empty_name",
            message: "site name cannot be empty".to_owned(),
        });
    }
    let site_url = validate_site_url(&body.site_url, "setup.site.invalid_site_url")?;
    let public_base_url =
        validate_site_url(&body.public_base_url, "setup.site.invalid_public_base_url")?;

    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.site.begin_transaction",
            source: source.into(),
        })?;
    settings::set_string(&transaction, "site.name", name)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.site.save",
            source,
        })?;
    settings::set_string(&transaction, "site.url", &site_url)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.site.save_url",
            source,
        })?;
    settings::set_string(&transaction, "site.public_base_url", &public_base_url)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.site.save_public_base_url",
            source,
        })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.site.commit",
            source: source.into(),
        })?;

    Ok(ok_response(json!({
        "configured": true,
        "name": name,
        "site_url": site_url,
        "public_base_url": public_base_url,
    })))
}

pub(crate) fn validate_site_url(value: &str, op: &'static str) -> Result<String, AppError> {
    let value = value.trim().trim_end_matches('/');
    let url = url::Url::parse(value).map_err(|_| AppError::Validation {
        op,
        message: "site URL must be an absolute http or https URL".to_owned(),
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(AppError::Validation {
            op,
            message: "site URL must be an absolute http or https URL".to_owned(),
        });
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_urls_are_absolute_http_urls() {
        assert_eq!(
            validate_site_url(" https://console.example/ ", "test").unwrap(),
            "https://console.example"
        );
        assert!(validate_site_url("/console", "test").is_err());
        assert!(validate_site_url("file:///tmp/console", "test").is_err());
    }
}
