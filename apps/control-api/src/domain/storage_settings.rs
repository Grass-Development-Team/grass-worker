use sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    domain::settings,
    infra::storage::{StorageConfig, StorageCredentials},
};

pub const CONFIG_KEY: &str = "storage.config";
pub const CREDENTIALS_KEY: &str = "storage.credentials";
pub const LEGACY_ROOT_KEY: &str = "storage.root";

const KEY_ID: &str = "platform-storage-v1";
const ASSOCIATED_DATA: &[u8] = b"grass-storage-credentials:v1";

#[derive(Debug, Clone)]
pub struct LoadedStorageSettings {
    pub config: StorageConfig,
    pub credentials: StorageCredentials,
    pub credentials_envelope: Option<serde_json::Value>,
}

pub async fn load_or_seed(
    db: &sea_orm::DatabaseConnection,
    legacy_root: &str,
    platform_secret: &str,
) -> anyhow::Result<LoadedStorageSettings> {
    if let Some(settings) = load(db, platform_secret).await? {
        return Ok(settings);
    }

    seed_local(db, legacy_root, platform_secret).await
}

pub(crate) async fn seed_local(
    db: &sea_orm::DatabaseConnection,
    legacy_root: &str,
    platform_secret: &str,
) -> anyhow::Result<LoadedStorageSettings> {
    let persisted_root = settings::get_setting(db, LEGACY_ROOT_KEY)
        .await?
        .and_then(|setting| setting.value.as_str().map(str::to_owned));
    let root = local_seed_root(persisted_root.as_deref(), legacy_root);
    let config = StorageConfig::local(root);
    save(db, &config, &StorageCredentials::default(), platform_secret).await?;
    Ok(LoadedStorageSettings {
        config,
        credentials: StorageCredentials::default(),
        credentials_envelope: None,
    })
}

pub async fn has_legacy_root<C: ConnectionTrait>(db: &C) -> anyhow::Result<bool> {
    Ok(settings::get_setting(db, LEGACY_ROOT_KEY)
        .await?
        .and_then(|setting| setting.value.as_str().map(str::trim).map(str::to_owned))
        .is_some_and(|root| !root.is_empty()))
}

fn local_seed_root(persisted_root: Option<&str>, configured_root: &str) -> String {
    persisted_root
        .map(str::trim)
        .filter(|root| !root.is_empty())
        .unwrap_or_else(|| configured_root.trim())
        .to_owned()
}

pub async fn load<C: ConnectionTrait>(
    db: &C,
    platform_secret: &str,
) -> anyhow::Result<Option<LoadedStorageSettings>> {
    let Some(config_setting) = settings::get_setting(db, CONFIG_KEY).await? else {
        return Ok(None);
    };
    let config: StorageConfig = serde_json::from_value(config_setting.value).map_err(|error| {
        anyhow::anyhow!("stored object storage configuration is invalid: {error}")
    })?;
    config.validate()?;

    let credentials_envelope = settings::get_setting(db, CREDENTIALS_KEY)
        .await?
        .and_then(|setting| (!setting.value.is_null()).then_some(setting.value));
    let credentials = credentials_envelope
        .as_ref()
        .map(|envelope| decrypt_credentials(platform_secret, envelope))
        .transpose()?
        .unwrap_or_default();
    Ok(Some(LoadedStorageSettings {
        config,
        credentials,
        credentials_envelope,
    }))
}

pub async fn save(
    db: &DatabaseConnection,
    config: &StorageConfig,
    credentials: &StorageCredentials,
    platform_secret: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    config.validate()?;
    let envelope = credentials
        .is_configured()
        .then(|| encrypt_credentials(platform_secret, credentials))
        .transpose()?;
    let transaction = db.begin().await?;
    save_raw(&transaction, config, envelope.as_ref()).await?;
    transaction.commit().await?;
    Ok(envelope)
}

pub async fn save_raw<C: ConnectionTrait>(
    db: &C,
    config: &StorageConfig,
    credentials_envelope: Option<&serde_json::Value>,
) -> anyhow::Result<()> {
    config.validate()?;
    settings::set_json(db, CONFIG_KEY, serde_json::to_value(config)?).await?;
    settings::set_secret_json(
        db,
        CREDENTIALS_KEY,
        credentials_envelope
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    )
    .await?;
    settings::set_string(db, LEGACY_ROOT_KEY, &config.local_root).await?;
    Ok(())
}

pub fn encrypt_credentials(
    platform_secret: &str,
    credentials: &StorageCredentials,
) -> anyhow::Result<serde_json::Value> {
    let plaintext = serde_json::to_vec(credentials)?;
    let envelope = grass_crypto::encrypt_secret(
        KEY_ID,
        &storage_key(platform_secret),
        &plaintext,
        ASSOCIATED_DATA,
    )?;
    Ok(serde_json::to_value(envelope)?)
}

pub fn decrypt_credentials(
    platform_secret: &str,
    value: &serde_json::Value,
) -> anyhow::Result<StorageCredentials> {
    let envelope: grass_crypto::AeadEnvelope = serde_json::from_value(value.clone())?;
    if envelope.key_id != KEY_ID {
        anyhow::bail!("stored object storage credential key is not supported");
    }
    let plaintext =
        grass_crypto::decrypt_secret(&envelope, &storage_key(platform_secret), ASSOCIATED_DATA)?;
    Ok(serde_json::from_slice(&plaintext)?)
}

fn storage_key(platform_secret: &str) -> [u8; 32] {
    Sha256::digest(format!("grass-object-storage:v1:{platform_secret}").as_bytes()).into()
}

pub fn public_config(config: &StorageConfig, credentials_configured: bool) -> serde_json::Value {
    json!({
        "backend": config.backend.as_str(),
        "local_root": config.local_root,
        "endpoint": config.endpoint,
        "region": config.region,
        "bucket": config.bucket,
        "prefix": config.prefix,
        "force_path_style": config.force_path_style,
        "allow_http": config.allow_http,
        "credentials_configured": credentials_configured,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_seed_prefers_legacy_root_and_ignores_blank_values() {
        assert_eq!(
            local_seed_root(Some("  /srv/legacy  "), "/srv/config"),
            "/srv/legacy"
        );
        assert_eq!(local_seed_root(Some("   "), "/srv/config"), "/srv/config");
        assert_eq!(local_seed_root(None, "/srv/config"), "/srv/config");
    }

    #[test]
    fn public_storage_config_redacts_all_credential_fields() {
        let config = StorageConfig {
            backend: crate::infra::storage::StorageBackendKind::R2,
            endpoint: "https://account.r2.cloudflarestorage.com".to_owned(),
            region: "auto".to_owned(),
            bucket: "artifacts".to_owned(),
            ..StorageConfig::default()
        };

        let public = public_config(&config, true);

        assert_eq!(public["credentials_configured"], true);
        for field in ["access_key_id", "secret_access_key", "session_token"] {
            assert!(public.get(field).is_none());
        }
    }

    #[test]
    fn credentials_are_encrypted_and_bound_to_storage_context() {
        let credentials = StorageCredentials {
            access_key_id: Some("access".to_owned()),
            secret_access_key: Some("secret".to_owned()),
            session_token: None,
        };
        let encrypted = encrypt_credentials("platform-secret", &credentials).unwrap();
        assert!(!encrypted.to_string().contains("secret"));
        assert_eq!(
            decrypt_credentials("platform-secret", &encrypted).unwrap(),
            credentials
        );
        assert!(decrypt_credentials("wrong-secret", &encrypted).is_err());
    }

    #[test]
    fn public_config_never_contains_credentials() {
        let value = public_config(&StorageConfig::default(), true);
        assert_eq!(value["credentials_configured"], true);
        assert!(value.get("secret_access_key").is_none());
        assert!(value.get("access_key_id").is_none());
    }
}
