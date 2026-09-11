use crate::{
    domain::certificate_settings,
    infra::error::{AppError, ok_response},
    state::ControlApiState,
};
use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::TransactionTrait;
use serde::Deserialize;
use serde_json::json;
pub async fn get(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.domain_https.get";
    let secret = state.config.read().unwrap().secrets.secret_key.clone();
    let settings = certificate_settings::load(super::database(&state, OP)?, &secret)
        .await
        .map_err(|_| AppError::Internal {
            op: OP,
            message: "Certificate settings could not be loaded".to_owned(),
        })?;
    Ok(ok_response(settings.view()))
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
    let db = super::database(&state, OP)?;
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
        .map_err(|_| AppError::Internal {
            op: OP,
            message: "Certificate settings could not be loaded".to_owned(),
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
        .map_err(|_| AppError::Internal {
            op: OP,
            message: "Certificate settings could not be saved".to_owned(),
        })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(settings.view()))
}
