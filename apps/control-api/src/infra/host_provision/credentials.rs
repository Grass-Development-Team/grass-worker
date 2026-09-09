//! Encrypted, write-only DNS provider configuration. Legacy JSON objects
//! remain readable and are upgraded under a row lock before provider use.

use sea_orm::sea_query::LockType;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, QuerySelect, TransactionTrait};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::HostProvisionError;
use crate::infra::database::entity::host_source;

const FORMAT: &str = "grass-host-source-config-v1";
const KEY_ID: &str = "host-source-platform-secret-v1";

pub fn encryption_key(platform_secret: &str) -> [u8; 32] {
    Sha256::digest(format!("grass-host-source-config:v1:{platform_secret}").as_bytes()).into()
}

fn associated_data(base_domain: &str, provider: Option<&str>) -> Vec<u8> {
    format!(
        "{FORMAT}:{}:{}",
        base_domain.trim_end_matches('.').to_ascii_lowercase(),
        provider.unwrap_or_default().to_ascii_lowercase()
    )
    .into_bytes()
}

pub fn is_encrypted(config: &Value) -> bool {
    config.get("_format").is_some()
}

pub fn config_keys(config: &Value) -> Vec<String> {
    if is_encrypted(config) {
        config
            .get("keys")
            .and_then(Value::as_array)
            .map(|keys| {
                keys.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    } else {
        config
            .as_object()
            .map(|object| object.keys().cloned().collect())
            .unwrap_or_default()
    }
}

fn credential_error() -> HostProvisionError {
    HostProvisionError::Provider(
        "host source credentials are unavailable or could not be authenticated".to_owned(),
    )
}

pub fn encrypt_config(
    key: &[u8; 32],
    base_domain: &str,
    provider: Option<&str>,
    config: &Value,
) -> Result<Value, HostProvisionError> {
    if !config.is_object() || is_encrypted(config) {
        return Err(credential_error());
    }
    if config.as_object().is_some_and(|object| object.is_empty()) {
        return Ok(json!({}));
    }
    let plaintext = serde_json::to_vec(config).map_err(|_| credential_error())?;
    let envelope = grass_crypto::encrypt_secret(
        KEY_ID,
        key,
        &plaintext,
        &associated_data(base_domain, provider),
    )
    .map_err(|_| credential_error())?;
    Ok(json!({"_format": FORMAT, "keys": config_keys(config), "encrypted": envelope}))
}

pub fn decrypt_config(
    key: &[u8; 32],
    base_domain: &str,
    provider: Option<&str>,
    config: &Value,
) -> Result<Value, HostProvisionError> {
    if !is_encrypted(config) {
        return config
            .is_object()
            .then(|| config.clone())
            .ok_or_else(credential_error);
    }
    if config.get("_format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(credential_error());
    }
    let envelope: grass_crypto::AeadEnvelope = serde_json::from_value(
        config
            .get("encrypted")
            .cloned()
            .ok_or_else(credential_error)?,
    )
    .map_err(|_| credential_error())?;
    if envelope.key_id != KEY_ID {
        return Err(credential_error());
    }
    let plaintext =
        grass_crypto::decrypt_secret(&envelope, key, &associated_data(base_domain, provider))
            .map_err(|_| credential_error())?;
    let value: Value = serde_json::from_slice(&plaintext).map_err(|_| credential_error())?;
    if !value.is_object() || is_encrypted(&value) {
        return Err(credential_error());
    }
    Ok(value)
}

/// Resolve runtime plaintext without exposing it through API views or SQL
/// parameters. Locking an old row also prevents upgrade from reverting a
/// concurrent administrator's credential rotation.
pub async fn runtime_source(
    db: &sea_orm::DatabaseConnection,
    key: &[u8; 32],
    source: &host_source::Model,
) -> Result<host_source::Model, HostProvisionError> {
    let mut source = source.clone();
    if !is_encrypted(&source.config)
        && source
            .config
            .as_object()
            .is_some_and(|object| !object.is_empty())
    {
        let transaction = db.begin().await.map_err(|_| credential_error())?;
        let current = host_source::Entity::find_by_id(source.id)
            .lock(LockType::Update)
            .one(&transaction)
            .await
            .map_err(|_| credential_error())?
            .ok_or_else(credential_error)?;
        source = if is_encrypted(&current.config) {
            current
        } else {
            let encrypted = encrypt_config(
                key,
                &current.base_domain,
                current.provider.as_deref(),
                &current.config,
            )?;
            let mut active: host_source::ActiveModel = current.into();
            active.config = Set(encrypted);
            active
                .update(&transaction)
                .await
                .map_err(|_| credential_error())?
        };
        transaction.commit().await.map_err(|_| credential_error())?;
    }
    source.config = decrypt_config(
        key,
        &source.base_domain,
        source.provider.as_deref(),
        &source.config,
    )?;
    Ok(source)
}

/// Provider errors can quote a submitted credential. Only sanitized text
/// may enter binding failure reasons, provision events, audit, or tracing.
pub fn redact_message(config: &Value, message: &str) -> String {
    let mut message = message.to_owned();
    for key in [
        "api_token",
        "secret_id",
        "secret_key",
        "access_key_id",
        "secret_access_key",
        "session_token",
    ] {
        if let Some(secret) = config
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            message = message.replace(secret, "[redacted]");
        }
    }
    message
}

pub(super) fn redact_error(config: &Value, error: HostProvisionError) -> HostProvisionError {
    match error {
        HostProvisionError::Provider(message) => {
            HostProvisionError::Provider(redact_message(config, &message))
        }
        HostProvisionError::UnsupportedSource(message) => {
            HostProvisionError::UnsupportedSource(redact_message(config, &message))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_encrypted_and_bound_to_the_provider_and_zone() {
        let key = encryption_key("test-platform-key");
        let config = json!({"api_token": "private-test-token", "zone_id": "test-zone", "record_type": "CNAME", "record_value": "ingress.example.com"});
        let first = encrypt_config(&key, "example.com", Some("cloudflare"), &config).unwrap();
        let second = encrypt_config(&key, "example.com", Some("cloudflare"), &config).unwrap();
        assert!(!first.to_string().contains("private-test-token"));
        assert!(!first.to_string().contains("test-zone"));
        assert_ne!(first["encrypted"]["nonce"], second["encrypted"]["nonce"]);
        assert_eq!(
            decrypt_config(&key, "EXAMPLE.COM.", Some("Cloudflare"), &first).unwrap(),
            config
        );
        assert!(
            decrypt_config(
                &encryption_key("wrong-key"),
                "example.com",
                Some("cloudflare"),
                &first
            )
            .is_err()
        );
        assert!(decrypt_config(&key, "other.example", Some("cloudflare"), &first).is_err());
        assert!(decrypt_config(&key, "example.com", Some("route53"), &first).is_err());
        assert_eq!(config_keys(&first), config_keys(&config));
        assert_eq!(
            decrypt_config(&key, "example.com", Some("cloudflare"), &config).unwrap(),
            config
        );
    }

    #[test]
    fn malformed_envelopes_do_not_fall_back_to_plaintext_and_errors_are_redacted() {
        let key = encryption_key("test-platform-key");
        let malformed = json!({"_format": "unknown", "api_token": "private-test-token"});
        let error =
            decrypt_config(&key, "example.com", Some("cloudflare"), &malformed).unwrap_err();
        assert!(!error.to_string().contains("private-test-token"));
        assert_eq!(
            redact_message(
                &json!({"api_token": "private-test-token", "secret_key": "key-material"}),
                "provider echoed private-test-token and key-material"
            ),
            "provider echoed [redacted] and [redacted]"
        );
    }

    fn source(config: Value) -> host_source::Model {
        host_source::Model {
            id: uuid::Uuid::now_v7(),
            kind: crate::infra::database::entity::HostSourceKind::DnsProvider,
            label: "test".to_owned(),
            base_domain: "example.com".to_owned(),
            region: "default".to_owned(),
            enabled: true,
            allows_auto_assign: true,
            is_default: false,
            provider: Some("cloudflare".to_owned()),
            config,
            deleted_at: None,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[tokio::test]
    async fn legacy_credentials_upgrade_under_lock_without_plaintext_sql_parameters() {
        let key = encryption_key("test-platform-key");
        let legacy = source(json!({"api_token": "private-test-token", "zone_id": "test-zone"}));
        let mut stored = legacy.clone();
        stored.config = encrypt_config(
            &key,
            &stored.base_domain,
            stored.provider.as_deref(),
            &legacy.config,
        )
        .unwrap();
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[legacy.clone()], [stored]])
            .into_connection();
        let runtime = runtime_source(&db, &key, &legacy).await.unwrap();
        assert_eq!(runtime.config, legacy.config);
        let sql = format!("{:?}", db.into_transaction_log());
        assert!(sql.contains("FOR UPDATE"));
        assert!(sql.contains("UPDATE"));
        assert!(sql.contains("ciphertext"));
        assert!(!sql.contains("private-test-token"));
        assert!(!sql.contains("test-zone"));
    }

    #[tokio::test]
    async fn legacy_upgrade_uses_a_concurrently_rotated_configuration() {
        let key = encryption_key("test-platform-key");
        let legacy = source(json!({"api_token": "old-test-token"}));
        let mut current = legacy.clone();
        current.config = encrypt_config(
            &key,
            &current.base_domain,
            current.provider.as_deref(),
            &json!({"api_token": "rotated-test-token"}),
        )
        .unwrap();
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[current]])
            .into_connection();
        let runtime = runtime_source(&db, &key, &legacy).await.unwrap();
        assert_eq!(runtime.config["api_token"], "rotated-test-token");
        let sql = format!("{:?}", db.into_transaction_log());
        assert!(!sql.contains("UPDATE "));
    }
}
