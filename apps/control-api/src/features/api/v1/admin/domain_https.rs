use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::TransactionTrait;
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::certificate_settings,
    infra::error::{AppError, ok_response},
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/domain-https", axum::routing::get(get).patch(update))
}

pub async fn get(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.domain_https.get";
    let secret = state.config.read().unwrap().secrets.secret_key.clone();
    let settings = certificate_settings::load(crate::infra::http::database(&state, OP)?, &secret)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.context("Certificate settings could not be loaded"),
        })?;
    Ok(ok_response(settings_response(&settings)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequest {
    pub issuer: String,
    pub eab_kid: Option<String>,
    pub eab_hmac_key: Option<String>,
}

pub async fn update(
    State(state): State<ControlApiState>,
    Json(body): Json<UpdateRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.domain_https.update";
    let db = crate::infra::http::database(&state, OP)?;
    let secret = state.config.read().unwrap().secrets.secret_key.clone();
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    // Serialize settings patches, including first creation of the row.
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    transaction
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT pg_advisory_xact_lock($1)",
            [84733191_i64.into()],
        ))
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let mut settings = certificate_settings::load(&transaction, &secret)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.context("Certificate settings could not be loaded"),
        })?;
    settings.issuer = body.issuer;
    match (
        body.eab_kid.filter(|v| !v.trim().is_empty()),
        body.eab_hmac_key.filter(|v| !v.trim().is_empty()),
    ) {
        (None, None) => {}
        (Some(kid), Some(key)) if kid.len() <= 1024 && key.len() <= 4096 => {
            settings.eab = json!({"eab_kid":kid.trim(),"eab_hmac_key":key.trim()})
        }
        _ => {
            return Err(AppError::Validation {
                op: OP,
                message: "Provide both ZeroSSL EAB credentials together".to_owned(),
            });
        }
    }
    settings.validate().map_err(|e| AppError::Validation {
        op: OP,
        message: e.to_string(),
    })?;
    certificate_settings::save(&transaction, &settings, &secret)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.context("Certificate settings could not be saved"),
        })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(settings_response(&settings)))
}

#[derive(serde::Serialize)]
struct CertificateSettingsResponse {
    issuer: String,
    zerossl_eab_configured: bool,
}

fn settings_response(
    settings: &certificate_settings::CertificateSettings,
) -> CertificateSettingsResponse {
    CertificateSettingsResponse {
        issuer: settings.issuer.clone(),
        zerossl_eab_configured: settings.eab.get("eab_kid").is_some()
            && settings.eab.get("eab_hmac_key").is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificate_settings_expose_configuration_flags_without_eab_secrets() {
        let settings = certificate_settings::CertificateSettings {
            issuer: "zerossl".to_owned(),
            eab: serde_json::json!({"eab_kid": "private-key-id", "eab_hmac_key": "private-key-value"}),
        };
        let value = serde_json::to_value(settings_response(&settings)).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"issuer": "zerossl", "zerossl_eab_configured": true})
        );
    }

    #[tokio::test]
    async fn failed_settings_load_retains_its_source_and_has_a_safe_response() {
        let state = ControlApiState::new(
            crate::infra::config::ControlApiConfig::default(),
            "unused.toml",
        );
        state
            .database
            .set(
                sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
                    .append_query_errors([sea_orm::DbErr::Custom("storage unavailable".to_owned())])
                    .into_connection(),
            )
            .ok()
            .unwrap();
        let error = match get(State(state)).await {
            Ok(_) => panic!("expected error"),
            Err(error) => error,
        };
        assert!(
            matches!(&error, AppError::Infrastructure { source, .. } if format!("{source:#}").contains("storage unavailable"))
        );
        let response = error.into_response();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(!body.contains("storage unavailable"));
    }
}
