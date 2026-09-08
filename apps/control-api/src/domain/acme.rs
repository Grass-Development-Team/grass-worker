//! ACME DNS-01 certificate lifecycle for regional ingress hosts.

use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, ExternalAccountKey,
    Identifier, LetsEncrypt, NewAccount, NewOrder, OrderStatus, RetryPolicy, ZeroSsl,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};
use serde_json::json;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    domain::authentication::authentication_key,
    infra::{database::entity::regional_ingress, host_provision::DnsProviderHostProvisioner},
};

const ACCOUNT_KEY_ID: &str = "regional-ingress-acme-account-v1";
const CERTIFICATE_KEY_ID: &str = "regional-ingress-certificate-v1";
const RENEW_BEFORE: Duration = Duration::days(30);
const DEFAULT_CERTIFICATE_LIFETIME: Duration = Duration::days(90);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileOutcome {
    Skipped,
    Issued,
}

pub async fn reconcile(
    db: &DatabaseConnection,
    ingress: &regional_ingress::Model,
    platform_secret: &str,
    force: bool,
) -> anyhow::Result<ReconcileOutcome> {
    if !ingress.enabled || !ingress.tls_enabled || ingress.certificate_issuer == "manual" {
        return Ok(ReconcileOutcome::Skipped);
    }
    if !force && ingress.certificate_status == "active" && !ingress.certificate_auto_renew {
        return Ok(ReconcileOutcome::Skipped);
    }
    if !force
        && ingress.certificate_status == "active"
        && ingress
            .certificate_expires_at
            .is_some_and(|expires| expires - OffsetDateTime::now_utc() > RENEW_BEFORE)
    {
        return Ok(ReconcileOutcome::Skipped);
    }

    let Some(provider) = ingress
        .dns_challenge_provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        set_certificate_state(
            db,
            ingress.id,
            "pending",
            Some("dns challenge provider is not configured"),
            None,
            None,
            None,
        )
        .await?;
        return Ok(ReconcileOutcome::Skipped);
    };

    set_certificate_state(db, ingress.id, "issuing", None, Some("pending"), None, None).await?;

    let (account, account_value) = match load_or_create_account(ingress, platform_secret).await {
        Ok(value) => value,
        Err(error) => {
            mark_failed(db, ingress.id, &error).await?;
            return Err(error);
        }
    };
    let account_envelope =
        match encrypt_json(platform_secret, ingress.id, ACCOUNT_KEY_ID, &account_value) {
            Ok(value) => value,
            Err(error) => {
                mark_failed(db, ingress.id, &error).await?;
                return Err(error);
            }
        };
    if let Err(error) = set_acme_account(db, ingress.id, account_envelope).await {
        mark_failed(db, ingress.id, &error).await?;
        return Err(error);
    }
    let result = issue(db, ingress, &account, provider).await;
    match result {
        Ok(IssuedCertificate {
            certificate_pem,
            private_key_pem,
        }) => {
            let now = OffsetDateTime::now_utc();
            let certificate_envelope = encrypt_json(
                platform_secret,
                ingress.id,
                CERTIFICATE_KEY_ID,
                &json!({
                    "certificate_pem": certificate_pem,
                    "private_key_pem": private_key_pem,
                }),
            )?;
            set_certificate_state(
                db,
                ingress.id,
                "active",
                None,
                Some("valid"),
                None,
                Some(certificate_envelope),
            )
            .await?;
            let fresh = regional_ingress::Entity::find_by_id(ingress.id)
                .one(db)
                .await?
                .ok_or_else(|| anyhow::anyhow!("regional ingress was removed during issuance"))?;
            let mut active: regional_ingress::ActiveModel = fresh.into();
            active.certificate_issued_at = Set(Some(now));
            active.certificate_expires_at = Set(Some(now + DEFAULT_CERTIFICATE_LIFETIME));
            active.update(db).await?;
            Ok(ReconcileOutcome::Issued)
        }
        Err(error) => {
            set_certificate_state(
                db,
                ingress.id,
                "failed",
                Some(&error.to_string()),
                Some("failed"),
                None,
                None,
            )
            .await?;
            Err(error)
        }
    }
}

pub async fn decrypt_bundle(
    ingress: &regional_ingress::Model,
    platform_secret: &str,
) -> anyhow::Result<Option<CertificateBundle>> {
    let Some(value) = ingress.certificate_bundle.as_ref() else {
        return Ok(None);
    };
    let value = decrypt_json(platform_secret, ingress.id, CERTIFICATE_KEY_ID, value)?;
    Ok(Some(serde_json::from_value(value)?))
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CertificateBundle {
    pub certificate_pem: String,
    pub private_key_pem: String,
}

async fn issue(
    db: &DatabaseConnection,
    ingress: &regional_ingress::Model,
    account: &Account,
    provider: &str,
) -> anyhow::Result<IssuedCertificate> {
    let config = &ingress.dns_challenge_config;
    let provisioner = DnsProviderHostProvisioner::new();
    let zone = config
        .get("zone")
        .and_then(serde_json::Value::as_str)
        .or_else(|| config.get("domain").and_then(serde_json::Value::as_str))
        .unwrap_or(&ingress.hostname);

    let identifiers = vec![Identifier::Dns(ingress.hostname.clone())];
    let mut order = account.new_order(&NewOrder::new(&identifiers)).await?;
    let mut record = None;
    {
        let mut authorizations = order.authorizations();
        while let Some(result) = authorizations.next().await {
            let mut authorization = result?;
            if authorization.status == AuthorizationStatus::Valid {
                continue;
            }
            let mut challenge = authorization
                .challenge(ChallengeType::Dns01)
                .ok_or_else(|| anyhow::anyhow!("ACME order has no DNS-01 challenge"))?;
            let name = format!("_acme-challenge.{}", challenge.identifier());
            let value = challenge.key_authorization().dns_value();
            set_dns_challenge(db, ingress.id, "pending", Some(&name), Some(&value)).await?;
            provisioner
                .ensure_txt_record(provider, config, zone, &name, &value)
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            record = Some((name, value));
            if let Err(error) = challenge.set_ready().await {
                if let Some((name, value)) = record.as_ref() {
                    let _ = provisioner
                        .remove_txt_record(provider, config, zone, name, value)
                        .await;
                }
                set_dns_challenge(db, ingress.id, "failed", None, None).await?;
                return Err(error.into());
            }
        }
    }
    let cleanup = async {
        if let Some((name, value)) = record.as_ref() {
            let _ = provisioner
                .remove_txt_record(provider, config, zone, name, value)
                .await;
        }
    };
    let ready = order.poll_ready(&RetryPolicy::default()).await;
    if !matches!(ready, Ok(OrderStatus::Ready)) {
        cleanup.await;
        set_dns_challenge(db, ingress.id, "failed", None, None).await?;
        anyhow::bail!("ACME order did not become ready: {:?}", ready.err());
    }
    let private_key_pem = match order.finalize().await {
        Ok(value) => value,
        Err(error) => {
            cleanup.await;
            set_dns_challenge(db, ingress.id, "failed", None, None).await?;
            return Err(error.into());
        }
    };
    let certificate_pem = match order.poll_certificate(&RetryPolicy::default()).await {
        Ok(value) => value,
        Err(error) => {
            cleanup.await;
            set_dns_challenge(db, ingress.id, "failed", None, None).await?;
            return Err(error.into());
        }
    };
    cleanup.await;
    set_dns_challenge(db, ingress.id, "valid", None, None).await?;
    Ok(IssuedCertificate {
        certificate_pem,
        private_key_pem,
    })
}

#[derive(Debug)]
struct IssuedCertificate {
    certificate_pem: String,
    private_key_pem: String,
}

async fn load_or_create_account(
    ingress: &regional_ingress::Model,
    platform_secret: &str,
) -> anyhow::Result<(Account, serde_json::Value)> {
    if let Some(value) = ingress.acme_account.as_ref() {
        let value = decrypt_json(platform_secret, ingress.id, ACCOUNT_KEY_ID, value)?;
        let credentials: AccountCredentials = serde_json::from_value(value.clone())?;
        let account = Account::builder()?.from_credentials(credentials).await?;
        return Ok((account, value));
    }

    let directory = match ingress.certificate_issuer.as_str() {
        "letsencrypt" => LetsEncrypt::Production.url(),
        "zerossl" => ZeroSsl::Production.url(),
        issuer => anyhow::bail!("unsupported ACME issuer {issuer}"),
    };
    let config = &ingress.dns_challenge_config;
    let contact = config
        .get("contact_email")
        .and_then(serde_json::Value::as_str)
        .map(|email| format!("mailto:{email}"));
    let contacts = contact.into_iter().collect::<Vec<_>>();
    let contact_refs = contacts.iter().map(String::as_str).collect::<Vec<_>>();
    let eab = external_account_key(config)?;
    if ingress.certificate_issuer == "zerossl" && eab.is_none() {
        anyhow::bail!("ZeroSSL requires eab_kid and eab_hmac_key");
    }
    let (account, credentials) = Account::builder()?
        .create(
            &NewAccount {
                contact: &contact_refs,
                terms_of_service_agreed: true,
                only_return_existing: false,
            },
            directory.to_owned(),
            eab.as_ref(),
        )
        .await?;
    Ok((account, serde_json::to_value(credentials)?))
}

fn external_account_key(config: &serde_json::Value) -> anyhow::Result<Option<ExternalAccountKey>> {
    let Some(kid) = config.get("eab_kid").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    let Some(hmac) = config
        .get("eab_hmac_key")
        .and_then(serde_json::Value::as_str)
    else {
        anyhow::bail!("eab_hmac_key is required when eab_kid is configured");
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(hmac)
        .or_else(|_| STANDARD.decode(hmac))
        .map_err(|_| anyhow::anyhow!("eab_hmac_key must be base64 encoded"))?;
    Ok(Some(ExternalAccountKey::new(kid.to_owned(), &bytes)))
}

fn associated_data(id: Uuid, key_id: &str) -> Vec<u8> {
    format!("grass-regional-ingress:{key_id}:{id}").into_bytes()
}

fn encrypt_json(
    platform_secret: &str,
    id: Uuid,
    key_id: &str,
    value: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    Ok(serde_json::to_value(grass_crypto::encrypt_secret(
        key_id,
        &authentication_key(platform_secret),
        &serde_json::to_vec(value)?,
        &associated_data(id, key_id),
    )?)?)
}

fn decrypt_json(
    platform_secret: &str,
    id: Uuid,
    key_id: &str,
    value: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let envelope: grass_crypto::AeadEnvelope = serde_json::from_value(value.clone())?;
    if envelope.key_id != key_id {
        anyhow::bail!("stored regional ingress secret has an unsupported key id");
    }
    let plaintext = grass_crypto::decrypt_secret(
        &envelope,
        &authentication_key(platform_secret),
        &associated_data(id, key_id),
    )?;
    Ok(serde_json::from_slice(&plaintext)?)
}

async fn set_certificate_state(
    db: &DatabaseConnection,
    id: Uuid,
    certificate_status: &str,
    certificate_error: Option<&str>,
    dns_status: Option<&str>,
    account: Option<serde_json::Value>,
    bundle: Option<serde_json::Value>,
) -> anyhow::Result<()> {
    let Some(item) = regional_ingress::Entity::find_by_id(id).one(db).await? else {
        anyhow::bail!("regional ingress not found");
    };
    let mut active: regional_ingress::ActiveModel = item.into();
    active.certificate_status = Set(certificate_status.to_owned());
    active.certificate_error = Set(certificate_error.map(str::to_owned));
    if let Some(status) = dns_status {
        active.dns_challenge_status = Set(status.to_owned());
    }
    if let Some(account) = account {
        active.acme_account = Set(Some(account));
    }
    if let Some(bundle) = bundle {
        active.certificate_bundle = Set(Some(bundle));
    }
    active.updated_at = Set(OffsetDateTime::now_utc());
    active.update(db).await?;
    Ok(())
}

async fn mark_failed(
    db: &DatabaseConnection,
    id: Uuid,
    error: &anyhow::Error,
) -> anyhow::Result<()> {
    set_certificate_state(
        db,
        id,
        "failed",
        Some(&error.to_string()),
        Some("failed"),
        None,
        None,
    )
    .await
}

async fn set_acme_account(
    db: &DatabaseConnection,
    id: Uuid,
    account: serde_json::Value,
) -> anyhow::Result<()> {
    let item = regional_ingress::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("regional ingress not found"))?;
    let mut active: regional_ingress::ActiveModel = item.into();
    active.acme_account = Set(Some(account));
    active.updated_at = Set(OffsetDateTime::now_utc());
    active.update(db).await?;
    Ok(())
}

async fn set_dns_challenge(
    db: &DatabaseConnection,
    id: Uuid,
    status: &str,
    name: Option<&str>,
    value: Option<&str>,
) -> anyhow::Result<()> {
    let item = regional_ingress::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("regional ingress not found"))?;
    let mut active: regional_ingress::ActiveModel = item.into();
    active.dns_challenge_status = Set(status.to_owned());
    active.dns_challenge_record_name = Set(name.map(str::to_owned));
    active.dns_challenge_record_value = Set(value.map(str::to_owned));
    active.updated_at = Set(OffsetDateTime::now_utc());
    active.update(db).await?;
    Ok(())
}

pub async fn sweep(db: &DatabaseConnection, platform_secret: &str) -> anyhow::Result<()> {
    let items = regional_ingress::Entity::find()
        .filter(regional_ingress::Column::Enabled.eq(true))
        .filter(regional_ingress::Column::TlsEnabled.eq(true))
        .filter(regional_ingress::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    for item in items {
        if let Err(error) = reconcile(db, &item, platform_secret, false).await {
            tracing::warn!(
                operation = "control_api.acme.reconcile_failed",
                ingress_id = %item.id,
                %error,
                "regional ingress certificate reconciliation failed"
            );
        }
    }
    Ok(())
}
