//! Persistent certificate material, eligibility and authoritative regional snapshots.

use anyhow::{Context, ensure};
use grass_node_protocol::{CertificateBundle, CertificateBundlesResponse, HttpChallenge};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QuerySelect, Set, TransactionTrait, sea_query::OnConflict,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use super::authentication::authentication_key;
use crate::infra::database::entity::{
    HostBindingKind, HostBindingStatus, HostReviewStatus, managed_certificate as cert,
    project_host_binding, regional_ingress,
};

pub const ACCOUNT_KEY: &str = "regional-ingress-acme-account-v1";
pub const BUNDLE_KEY: &str = "regional-ingress-certificate-v1";
const CONFIG_KEY: &str = "regional-ingress-provider-v1";

pub fn encrypt(secret: &str, id: Uuid, key: &str, value: &Value) -> anyhow::Result<Value> {
    Ok(serde_json::to_value(grass_crypto::encrypt_secret(
        key,
        &authentication_key(secret),
        &serde_json::to_vec(value)?,
        format!("grass-regional-ingress:{key}:{id}").as_bytes(),
    )?)?)
}

pub fn decrypt(secret: &str, id: Uuid, key: &str, value: &Value) -> anyhow::Result<Value> {
    let envelope: grass_crypto::AeadEnvelope = serde_json::from_value(value.clone())?;
    ensure!(envelope.key_id == key, "unsupported stored secret key id");
    let plaintext = grass_crypto::decrypt_secret(
        &envelope,
        &authentication_key(secret),
        format!("grass-regional-ingress:{key}:{id}").as_bytes(),
    )?;
    Ok(serde_json::from_slice(&plaintext)?)
}

pub fn config(ingress: &regional_ingress::Model, secret: &str) -> anyhow::Result<Value> {
    if ingress.dns_challenge_config.get("key_id").is_some() {
        decrypt(
            secret,
            ingress.id,
            CONFIG_KEY,
            &ingress.dns_challenge_config,
        )
    } else {
        // Previous releases stored plaintext. The certificate sweep upgrades it in place.
        Ok(ingress.dns_challenge_config.clone())
    }
}

pub fn seal_config(id: Uuid, value: &Value, secret: &str) -> anyhow::Result<Value> {
    ensure!(value.is_object(), "DNS configuration must be an object");
    ensure!(
        serde_json::to_vec(value)?.len() <= 16_384,
        "DNS configuration exceeds 16 KiB"
    );
    encrypt(secret, id, CONFIG_KEY, value)
}

pub fn merge_config(mut previous: Value, patch: &Value) -> anyhow::Result<Value> {
    let previous = previous
        .as_object_mut()
        .context("stored DNS configuration is invalid")?;
    for (key, value) in patch
        .as_object()
        .context("DNS configuration must be an object")?
    {
        ensure!(key.len() <= 128, "DNS configuration key exceeds 128 bytes");
        if value.is_null() {
            previous.remove(key);
        } else if value.as_str() != Some("") {
            previous.insert(key.clone(), value.clone());
        }
    }
    Ok(Value::Object(previous.clone()))
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct PemBundle {
    pub certificate_pem: String,
    pub private_key_pem: String,
}

pub struct CertificateValidity {
    pub issued_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
    pub revision: String,
}

pub fn validate_pem(
    hostname: &str,
    bundle: &PemBundle,
    now: OffsetDateTime,
) -> anyhow::Result<CertificateValidity> {
    ensure!(
        bundle.certificate_pem.len() <= 65_536 && bundle.private_key_pem.len() <= 16_384,
        "certificate material exceeds size limit"
    );
    let chain = rustls_pemfile::certs(&mut bundle.certificate_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()?;
    let leaf = chain.first().context("certificate chain is empty")?;
    let (_, certificate) = x509_parser::parse_x509_certificate(leaf.as_ref())
        .map_err(|_| anyhow::anyhow!("invalid X.509 certificate"))?;
    let mut issued_at =
        OffsetDateTime::from_unix_timestamp(certificate.validity().not_before.timestamp())?;
    let mut expires_at =
        OffsetDateTime::from_unix_timestamp(certificate.validity().not_after.timestamp())?;
    ensure!(
        issued_at <= now && expires_at > now,
        "certificate is not currently valid"
    );
    for entry in chain.iter().skip(1) {
        let (_, certificate) = x509_parser::parse_x509_certificate(entry.as_ref())
            .map_err(|_| anyhow::anyhow!("invalid certificate chain entry"))?;
        let starts =
            OffsetDateTime::from_unix_timestamp(certificate.validity().not_before.timestamp())?;
        let ends =
            OffsetDateTime::from_unix_timestamp(certificate.validity().not_after.timestamp())?;
        ensure!(
            starts <= now && ends > now,
            "certificate chain is not currently valid"
        );
        issued_at = issued_at.max(starts);
        expires_at = expires_at.min(ends);
    }
    let parsed = rustls::server::ParsedCertificate::try_from(leaf)?;
    rustls::client::verify_server_name(
        &parsed,
        &rustls::pki_types::ServerName::try_from(hostname.to_owned())?,
    )?;
    let key = rustls_pemfile::private_key(&mut bundle.private_key_pem.as_bytes())?
        .context("private key is missing")?;
    rustls::sign::CertifiedKey::new(chain, rustls::crypto::ring::sign::any_supported_type(&key)?)
        .keys_match()?;
    Ok(CertificateValidity {
        issued_at,
        expires_at,
        revision: hex::encode(Sha256::digest(bundle.certificate_pem.as_bytes())),
    })
}

pub fn binding_eligible(binding: &project_host_binding::Model) -> bool {
    binding.deleted_at.is_none()
        && matches!(binding.kind, HostBindingKind::Custom)
        && matches!(binding.status, HostBindingStatus::Active)
        && binding.ownership_status == "verified"
        && matches!(
            binding.review_status,
            HostReviewStatus::Approved | HostReviewStatus::NotRequired
        )
}

pub async fn eligible<C: ConnectionTrait>(
    db: &C,
    item: &cert::Model,
    ingress: &regional_ingress::Model,
) -> anyhow::Result<bool> {
    let Some(ingress) = regional_ingress::Entity::find_by_id(ingress.id)
        .one(db)
        .await?
    else {
        return Ok(false);
    };
    if !ingress.enabled
        || !ingress.tls_enabled
        || ingress.deleted_at.is_some()
        || item.ingress_id != ingress.id
    {
        return Ok(false);
    }
    if let Some(id) = item.host_binding_id {
        Ok(project_host_binding::Entity::find_by_id(id)
            .one(db)
            .await?
            .is_some_and(|binding| {
                binding_eligible(&binding)
                    && binding.host == item.hostname
                    && binding.region == ingress.region
            }))
    } else {
        Ok(item.hostname == ingress.hostname)
    }
}

pub async fn ensure_record(
    db: &DatabaseConnection,
    ingress: &regional_ingress::Model,
    binding: Option<&project_host_binding::Model>,
) -> anyhow::Result<cert::Model> {
    let transaction = db.begin().await?;
    let ingress = regional_ingress::Entity::find_by_id(ingress.id)
        .lock_exclusive()
        .one(&transaction)
        .await?
        .context("regional ingress no longer exists")?;
    let item = ensure_record_inner(&transaction, &ingress, binding).await?;
    transaction.commit().await?;
    Ok(item)
}

/// Upgrade local legacy state while holding the same ingress/certificate locks
/// used by administrator edits. Only the ID comes from the sweep's earlier list.
pub async fn prepare_regional_sweep(
    db: &DatabaseConnection,
    ingress_id: Uuid,
    secret: &str,
) -> anyhow::Result<Option<regional_ingress::Model>> {
    let transaction = db.begin().await?;
    let Some(mut ingress) = regional_ingress::Entity::find_by_id(ingress_id)
        .lock_exclusive()
        .one(&transaction)
        .await?
    else {
        return Ok(None);
    };
    if ingress.dns_challenge_config.get("key_id").is_none() {
        let sealed = seal_config(ingress.id, &ingress.dns_challenge_config, secret)?;
        let mut active: regional_ingress::ActiveModel = ingress.into();
        active.dns_challenge_config = Set(sealed);
        ingress = active.update(&transaction).await?;
    }
    if ingress.enabled && ingress.tls_enabled && ingress.deleted_at.is_none() {
        let regional = ensure_record_inner(&transaction, &ingress, None).await?;
        // An in-flight order owns its status and material until it releases its lease.
        if !regional
            .lease_until
            .is_some_and(|until| until > OffsetDateTime::now_utc())
        {
            let mut active: cert::ActiveModel = regional.clone().into();
            let auto_renew = regional.issuer != "manual" && ingress.certificate_auto_renew;
            let mut changed = regional.auto_renew != auto_renew;
            if changed {
                active.auto_renew = Set(auto_renew);
            }
            if regional.bundle.is_none()
                && let Some(old) = &ingress.certificate_bundle
            {
                let restored = (|| -> anyhow::Result<_> {
                    let bundle: PemBundle =
                        serde_json::from_value(decrypt(secret, ingress.id, BUNDLE_KEY, old)?)?;
                    validate_pem(&ingress.hostname, &bundle, OffsetDateTime::now_utc())
                })();
                match restored {
                    Ok(validity) => {
                        active.bundle = Set(Some(old.clone()));
                        active.revision = Set(validity.revision);
                        active.issued_at = Set(Some(validity.issued_at));
                        active.expires_at = Set(Some(validity.expires_at));
                        active.status = Set("active".to_owned());
                        changed = true;
                    }
                    Err(_) => {
                        tracing::warn!(operation="control_api.certificate.invalid_legacy_bundle",ingress_id=%ingress.id,"legacy certificate could not be restored; current certificate state is retained");
                    }
                }
            }
            if changed {
                active.update(&transaction).await?;
            }
        }
    }
    transaction.commit().await?;
    Ok(Some(ingress))
}

async fn ensure_record_inner<C: ConnectionTrait>(
    db: &C,
    ingress: &regional_ingress::Model,
    binding: Option<&project_host_binding::Model>,
) -> anyhow::Result<cert::Model> {
    let id = binding.map_or(ingress.id, |b| b.id);
    let hostname = binding.map_or(&ingress.hostname, |b| &b.host);
    if let Some(item) = cert::Entity::find_by_id(id)
        .lock_exclusive()
        .one(db)
        .await?
    {
        let changed = item.hostname != *hostname
            || item.ingress_id != ingress.id
            || (item.issuer != "manual" && item.issuer != ingress.certificate_issuer);
        if !changed {
            return Ok(item);
        }
        let mut active: cert::ActiveModel = item.clone().into();
        active.hostname = Set(hostname.clone());
        active.ingress_id = Set(ingress.id);
        active.issuer = Set(ingress.certificate_issuer.clone());
        active.acme_account = Set(None);
        active.status = Set("pending".to_owned());
        active.error = Set(None);
        active.retry_at = Set(None);
        active.failure_count = Set(0);
        active.lease_until = Set(None);
        active.generation = Set(Uuid::now_v7());
        active.challenge_token = Set(None);
        active.challenge_value = Set(None);
        active.challenge_expires_at = Set(None);
        if item.hostname != *hostname {
            active.bundle = Set(None);
            active.revision = Set(String::new());
            active.expires_at = Set(None);
            active.issued_at = Set(None);
        }
        return Ok(active.update(db).await?);
    }
    let active = cert::ActiveModel {
        id: Set(id),
        ingress_id: Set(ingress.id),
        host_binding_id: Set(binding.map(|b| b.id)),
        hostname: Set(hostname.clone()),
        issuer: Set(ingress.certificate_issuer.clone()),
        challenge_method: Set(if binding.is_some() { "http01" } else { "dns01" }.to_owned()),
        auto_renew: Set(ingress.certificate_auto_renew),
        generation: Set(Uuid::now_v7()),
        ..Default::default()
    };
    cert::Entity::insert(active)
        .on_conflict(OnConflict::column(cert::Column::Id).do_nothing().to_owned())
        .try_insert()
        .exec(db)
        .await?;
    cert::Entity::find_by_id(id)
        .one(db)
        .await?
        .context("certificate record disappeared")
}

pub async fn queue(db: &DatabaseConnection, item: &cert::Model) -> anyhow::Result<cert::Model> {
    let transaction = db.begin().await?;
    let item = cert::Entity::find_by_id(item.id)
        .lock_exclusive()
        .one(&transaction)
        .await?
        .context("certificate no longer exists")?;
    ensure!(
        item.issuer != "manual",
        "manual certificates must be imported"
    );
    let now = OffsetDateTime::now_utc();
    ensure!(
        !item.lease_until.is_some_and(|until| until > now),
        "certificate issuance is already in progress"
    );
    ensure!(
        !item.retry_at.is_some_and(|until| until > now),
        "certificate retry is temporarily delayed after failure"
    );
    let mut active: cert::ActiveModel = item.clone().into();
    active.status = Set("pending".to_owned());
    active.retry_at = Set(None);
    active.error = Set(None);
    active.generation = Set(Uuid::now_v7());
    let item = active.update(&transaction).await?;
    transaction.commit().await?;
    Ok(item)
}

pub async fn import(
    db: &DatabaseConnection,
    item: &cert::Model,
    bundle: PemBundle,
    secret: &str,
) -> anyhow::Result<cert::Model> {
    let transaction = db.begin().await?;
    let item = cert::Entity::find_by_id(item.id)
        .lock_exclusive()
        .one(&transaction)
        .await?
        .context("certificate no longer exists")?;
    ensure!(
        !item
            .lease_until
            .is_some_and(|until| until > OffsetDateTime::now_utc()),
        "wait for the in-progress certificate attempt before importing"
    );
    let validity = validate_pem(&item.hostname, &bundle, OffsetDateTime::now_utc())?;
    let mut active: cert::ActiveModel = item.clone().into();
    active.issuer = Set("manual".to_owned());
    active.auto_renew = Set(false);
    active.status = Set("active".to_owned());
    active.error = Set(None);
    active.bundle = Set(Some(encrypt(
        secret,
        item.id,
        BUNDLE_KEY,
        &serde_json::to_value(bundle)?,
    )?));
    active.issued_at = Set(Some(validity.issued_at));
    active.expires_at = Set(Some(validity.expires_at));
    active.revision = Set(validity.revision);
    active.generation = Set(Uuid::now_v7());
    active.lease_until = Set(None);
    active.retry_at = Set(None);
    active.failure_count = Set(0);
    active.challenge_token = Set(None);
    active.challenge_value = Set(None);
    active.challenge_expires_at = Set(None);
    let updated = active.update(&transaction).await?;
    transaction.commit().await?;
    sync_regional_status(db, &updated).await?;
    Ok(updated)
}

pub async fn sync_regional_status(
    db: &DatabaseConnection,
    item: &cert::Model,
) -> anyhow::Result<()> {
    if item.host_binding_id.is_some() {
        return Ok(());
    }
    if let Some(ingress) = regional_ingress::Entity::find_by_id(item.ingress_id)
        .one(db)
        .await?
    {
        if ingress.hostname != item.hostname {
            return Ok(());
        }
        let mut active: regional_ingress::ActiveModel = ingress.into();
        active.certificate_status = Set(item.status.clone());
        active.certificate_error = Set(item.error.clone());
        active.certificate_issued_at = Set(item.issued_at);
        active.certificate_expires_at = Set(item.expires_at);
        active.update(db).await?;
    }
    Ok(())
}

pub fn delegation_target(item: &cert::Model, ingress: &regional_ingress::Model) -> String {
    format!("_acme-{}.{}", item.id.simple(), ingress.hostname)
}

pub fn view(item: Option<&cert::Model>, ingress: &regional_ingress::Model) -> Value {
    let mut view = match item {
        Some(item) => {
            json!({ "enabled": ingress.enabled && ingress.tls_enabled, "status": item.status, "issuer": item.issuer, "challenge_method": item.challenge_method, "auto_renew": item.auto_renew, "issued_at": crate::infra::http::timestamps::ts(item.issued_at), "expires_at": crate::infra::http::timestamps::ts(item.expires_at), "error": item.error, "retry_at": crate::infra::http::timestamps::ts(item.retry_at), "revision": item.revision, "dns_delegation_name": format!("_acme-challenge.{}", item.hostname), "dns_delegation_target": delegation_target(item, ingress) })
        }
        None => {
            json!({"enabled": ingress.enabled && ingress.tls_enabled, "status":"pending", "issuer":ingress.certificate_issuer, "challenge_method":"http01", "auto_renew":ingress.certificate_auto_renew, "issued_at":null, "expires_at":null, "error":null, "retry_at":null, "revision":"", "dns_delegation_name":null, "dns_delegation_target":null})
        }
    };
    view["regional_issuer"] = json!(ingress.certificate_issuer);
    if !ingress.enabled || !ingress.tls_enabled || ingress.deleted_at.is_some() {
        view["status"] = json!("disabled");
    }
    view
}

pub fn challenge_revision(challenges: &[HttpChallenge]) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(challenges).expect("challenge snapshot serializes"),
    ))
}

pub async fn snapshot(
    db: &DatabaseConnection,
    region: &str,
    secret: &str,
) -> anyhow::Result<CertificateBundlesResponse> {
    let mut bundles = Vec::new();
    let mut challenges = Vec::new();
    if let Some(ingress) = super::ingress::get_enabled_by_region(db, region).await? {
        let items = cert::Entity::find()
            .filter(cert::Column::IngressId.eq(ingress.id))
            .all(db)
            .await?;
        let now = OffsetDateTime::now_utc();
        for item in items {
            if !eligible(db, &item, &ingress).await? {
                continue;
            }
            if let (Some(token), Some(value), Some(expires)) = (
                &item.challenge_token,
                &item.challenge_value,
                item.challenge_expires_at,
            ) && expires > now
            {
                challenges.push(HttpChallenge {
                    hostname: item.hostname.clone(),
                    token: token.clone(),
                    key_authorization: value.clone(),
                    expires_at_unix: expires.unix_timestamp(),
                });
            }
            if let Some(value) = &item.bundle
                && item.expires_at.is_some_and(|expiry| expiry > now)
            {
                let parsed = (|| -> anyhow::Result<_> {
                    let pem: PemBundle =
                        serde_json::from_value(decrypt(secret, item.id, BUNDLE_KEY, value)?)?;
                    let validity = validate_pem(&item.hostname, &pem, now)?;
                    Ok((pem, validity))
                })();
                let Ok((pem, validity)) = parsed else {
                    tracing::warn!(operation="control_api.certificate.invalid_stored_bundle",certificate_id=%item.id,"invalid stored certificate omitted from regional snapshot");
                    continue;
                };
                bundles.push(CertificateBundle {
                    ingress_id: item.id,
                    hostname: item.hostname,
                    certificate_pem: pem.certificate_pem,
                    private_key_pem: pem.private_key_pem,
                    issued_at_unix: Some(validity.issued_at.unix_timestamp()),
                    revision: validity.revision,
                    expires_at_unix: Some(validity.expires_at.unix_timestamp()),
                });
            }
        }
    }
    bundles.sort_by(|a, b| a.hostname.cmp(&b.hostname));
    challenges.sort_by(|a, b| (&a.hostname, &a.token).cmp(&(&b.hostname, &b.token)));
    let challenge_revision = challenge_revision(&challenges);
    Ok(CertificateBundlesResponse {
        bundles,
        challenges,
        challenge_revision,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    #[tokio::test]
    async fn sweep_preparation_preserves_locked_rotated_credentials_and_manual_certificate() {
        let stale = ingress_fixture();
        let mut current = stale.clone();
        current.dns_challenge_config =
            seal_config(current.id, &json!({"api_token":"rotated-token"}), "secret").unwrap();
        // The certificate import has committed; its admin endpoint has not yet
        // changed the ingress's old automatic-renewal preference.
        let mut imported = certificate_fixture(&current);
        imported.issuer = "manual".to_owned();
        imported.auto_renew = false;
        imported.bundle = Some(json!({"current_manual_material":true}));
        imported.status = "active".to_owned();
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([vec![current.clone()]])
            .append_query_results([vec![imported]])
            .into_connection();
        let prepared = prepare_regional_sweep(&db, stale.id, "secret")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            config(&prepared, "secret").unwrap(),
            json!({"api_token":"rotated-token"})
        );
        let log = db.into_transaction_log();
        assert_eq!(log.len(), 1);
        assert_eq!(
            log[0]
                .statements()
                .iter()
                .filter(|s| s.sql.contains("FOR UPDATE"))
                .count(),
            2
        );
        assert!(
            !log[0]
                .statements()
                .iter()
                .any(|s| s.sql.starts_with("UPDATE "))
        );
    }

    #[tokio::test]
    async fn sweep_preparation_syncs_only_the_locked_current_renewal_preference() {
        let stale = ingress_fixture();
        let mut current = stale.clone();
        current.certificate_auto_renew = false;
        current.dns_challenge_config = seal_config(current.id, &json!({}), "secret").unwrap();
        let old = certificate_fixture(&current);
        let mut updated = old.clone();
        updated.auto_renew = false;
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([vec![current]])
            .append_query_results([vec![old], vec![updated]])
            .into_connection();
        assert!(
            !prepare_regional_sweep(&db, stale.id, "secret")
                .await
                .unwrap()
                .unwrap()
                .certificate_auto_renew
        );
        let log = db.into_transaction_log();
        let updates = log
            .iter()
            .flat_map(|t| t.statements())
            .filter(|s| s.sql.starts_with("UPDATE "))
            .collect::<Vec<_>>();
        assert_eq!(updates.len(), 1);
        assert!(updates[0].sql.contains("SET \"auto_renew\" ="));
        assert_eq!(
            updates[0].values.as_ref().unwrap().0[0],
            sea_orm::Value::Bool(Some(false))
        );
    }

    #[tokio::test]
    async fn sweep_preparation_does_not_restore_legacy_material_over_an_in_flight_order() {
        let mut ingress = ingress_fixture();
        ingress.dns_challenge_config = seal_config(ingress.id, &json!({}), "secret").unwrap();
        ingress.certificate_bundle = Some(json!({"old_envelope":"not-yet-read"}));
        let mut current = certificate_fixture(&ingress);
        current.status = "issuing".to_owned();
        current.lease_until = Some(OffsetDateTime::now_utc() + time::Duration::minutes(1));
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([vec![ingress.clone()]])
            .append_query_results([vec![current]])
            .into_connection();
        prepare_regional_sweep(&db, ingress.id, "secret")
            .await
            .unwrap();
        assert!(
            !db.into_transaction_log()
                .iter()
                .flat_map(|t| t.statements())
                .any(|s| s.sql.starts_with("UPDATE "))
        );
    }
    #[tokio::test]
    async fn corrupt_bundle_does_not_block_another_domains_withdrawal() {
        let ingress = ingress_fixture();
        let mut corrupt = certificate_fixture(&ingress);
        corrupt.bundle = Some(json!({"invalid":true}));
        corrupt.expires_at = Some(OffsetDateTime::now_utc() + time::Duration::days(30));
        let mut binding = binding_fixture();
        binding.deleted_at = Some(OffsetDateTime::now_utc());
        let mut removed = certificate_fixture(&ingress);
        removed.id = binding.id;
        removed.host_binding_id = Some(binding.id);
        removed.hostname = binding.host.clone();
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([vec![ingress.clone()]])
            .append_query_results([vec![corrupt, removed]])
            .append_query_results([vec![ingress.clone()], vec![ingress]])
            .append_query_results([vec![binding]])
            .into_connection();
        let snapshot = snapshot(&db, "eu", "secret").await.unwrap();
        assert!(snapshot.bundles.is_empty());
        assert!(snapshot.challenges.is_empty());
    }

    #[tokio::test]
    async fn regional_hostname_change_preserves_custom_manual_certificate_override() {
        let ingress = ingress_fixture();
        let binding = binding_fixture();
        let mut item = certificate_fixture(&ingress);
        item.id = binding.id;
        item.host_binding_id = Some(binding.id);
        item.hostname = binding.host.clone();
        item.issuer = "manual".to_owned();
        item.status = "active".to_owned();
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([vec![ingress.clone()]])
            .append_query_results([vec![item.clone()]])
            .into_connection();
        let found = ensure_record(&db, &ingress, Some(&binding)).await.unwrap();
        assert_eq!(found, item);
    }

    #[test]
    fn valid_leaf_does_not_hide_expired_chain_certificate() {
        let leaf = rcgen::generate_simple_self_signed(vec!["site.example.org".to_owned()]).unwrap();
        let now = OffsetDateTime::now_utc();
        let mut params =
            rcgen::CertificateParams::new(vec!["issuer.example.org".to_owned()]).unwrap();
        params.not_before = now - time::Duration::days(3);
        params.not_after = now - time::Duration::days(1);
        let intermediate = params
            .self_signed(&rcgen::KeyPair::generate().unwrap())
            .unwrap();
        let bundle = PemBundle {
            certificate_pem: format!("{}{}", leaf.cert.pem(), intermediate.pem()),
            private_key_pem: leaf.signing_key.serialize_pem(),
        };
        assert!(validate_pem("site.example.org", &bundle, now).is_err());
    }
    pub(crate) fn ingress_fixture() -> regional_ingress::Model {
        regional_ingress::Model {
            id: Uuid::now_v7(),
            region: "eu".to_owned(),
            hostname: "entry.example.org".to_owned(),
            enabled: true,
            health_check_path: "/_grass/health".to_owned(),
            health_check_interval_seconds: 30,
            origin_host_preservation: true,
            tls_enabled: true,
            certificate_issuer: "letsencrypt".to_owned(),
            certificate_auto_renew: true,
            certificate_status: "pending".to_owned(),
            certificate_expires_at: None,
            certificate_error: None,
            dns_challenge_provider: None,
            dns_challenge_config: json!({}),
            dns_challenge_status: "not_configured".to_owned(),
            dns_challenge_record_name: None,
            dns_challenge_record_value: None,
            acme_account: None,
            certificate_bundle: None,
            certificate_issued_at: None,
            deleted_at: None,
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        }
    }
    pub(crate) fn certificate_fixture(ingress: &regional_ingress::Model) -> cert::Model {
        cert::Model {
            id: ingress.id,
            ingress_id: ingress.id,
            host_binding_id: None,
            hostname: ingress.hostname.clone(),
            issuer: "letsencrypt".to_owned(),
            challenge_method: "dns01".to_owned(),
            auto_renew: true,
            status: "pending".to_owned(),
            error: None,
            bundle: None,
            acme_account: None,
            revision: String::new(),
            issued_at: None,
            expires_at: None,
            retry_at: None,
            failure_count: 0,
            lease_until: None,
            generation: Uuid::now_v7(),
            challenge_token: None,
            challenge_value: None,
            challenge_expires_at: None,
            dns_record_name: None,
            dns_record_value: None,
            dns_cleanup: None,
            updated_at: OffsetDateTime::now_utc(),
        }
    }
    pub(crate) fn binding_fixture() -> project_host_binding::Model {
        project_host_binding::Model {
            id: Uuid::now_v7(),
            project_id: Uuid::now_v7(),
            team_id: Uuid::now_v7(),
            host_source_id: None,
            host: "site.example.org".to_owned(),
            region: "eu".to_owned(),
            kind: HostBindingKind::Custom,
            environment: crate::infra::database::entity::HostBindingEnvironment::Production,
            status: HostBindingStatus::Active,
            failure_reason: None,
            is_primary: false,
            review_status: HostReviewStatus::Approved,
            reviewed_by_user_id: None,
            reviewed_at: None,
            review_reason: None,
            ownership_status: "verified".to_owned(),
            ownership_checked_at: None,
            ownership_error: None,
            deleted_at: None,
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        }
    }

    #[test]
    fn ownership_review_and_lifecycle_are_independent_gates() {
        let binding = binding_fixture();
        assert!(binding_eligible(&binding));
        let mut pending = binding.clone();
        pending.ownership_status = "pending".to_owned();
        assert!(!binding_eligible(&pending));
        let mut rejected = binding.clone();
        rejected.review_status = HostReviewStatus::Rejected;
        assert!(!binding_eligible(&rejected));
        let mut disabled = binding.clone();
        disabled.status = HostBindingStatus::Disabled;
        assert!(!binding_eligible(&disabled));
        let mut deleted = binding;
        deleted.deleted_at = Some(OffsetDateTime::now_utc());
        assert!(!binding_eligible(&deleted));
    }

    #[tokio::test]
    async fn failed_renewal_keeps_valid_certificate_and_expired_challenges_are_withdrawn() {
        let ingress = ingress_fixture();
        let mut item = certificate_fixture(&ingress);
        let generated = rcgen::generate_simple_self_signed(vec![ingress.hostname.clone()]).unwrap();
        let bundle = PemBundle {
            certificate_pem: generated.cert.pem(),
            private_key_pem: generated.signing_key.serialize_pem(),
        };
        let validity = validate_pem(&ingress.hostname, &bundle, OffsetDateTime::now_utc()).unwrap();
        item.bundle = Some(
            encrypt(
                "secret",
                item.id,
                BUNDLE_KEY,
                &serde_json::to_value(bundle).unwrap(),
            )
            .unwrap(),
        );
        item.expires_at = Some(validity.expires_at);
        item.status = "failed".to_owned();
        item.challenge_token = Some("expired".to_owned());
        item.challenge_value = Some("expired.value".to_owned());
        item.challenge_expires_at = Some(OffsetDateTime::now_utc() - time::Duration::seconds(1));
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([vec![ingress.clone()]])
            .append_query_results([vec![item]])
            .append_query_results([vec![ingress]])
            .into_connection();
        let snapshot = snapshot(&db, "eu", "secret").await.unwrap();
        assert_eq!(snapshot.bundles.len(), 1);
        assert!(snapshot.challenges.is_empty());
        assert_eq!(snapshot.challenge_revision, challenge_revision(&[]));
        assert_eq!(snapshot.bundles[0].revision, validity.revision);
    }

    #[tokio::test]
    async fn custom_domain_disablement_withdraws_certificate_and_http_challenge() {
        let ingress = ingress_fixture();
        let mut binding = binding_fixture();
        binding.status = HostBindingStatus::Disabled;
        let mut item = certificate_fixture(&ingress);
        item.id = binding.id;
        item.host_binding_id = Some(binding.id);
        item.hostname = binding.host.clone();
        item.challenge_token = Some("token".to_owned());
        item.challenge_value = Some("token.thumbprint".to_owned());
        item.challenge_expires_at = Some(OffsetDateTime::now_utc() + time::Duration::minutes(5));
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([vec![ingress.clone()]])
            .append_query_results([vec![item]])
            .append_query_results([vec![ingress]])
            .append_query_results([vec![binding]])
            .into_connection();
        let snapshot = snapshot(&db, "eu", "secret").await.unwrap();
        assert!(snapshot.bundles.is_empty());
        assert!(snapshot.challenges.is_empty());
    }

    #[tokio::test]
    async fn custom_http_challenge_is_published_before_deployment_exists() {
        let ingress = ingress_fixture();
        let binding = binding_fixture();
        let mut item = certificate_fixture(&ingress);
        item.id = binding.id;
        item.host_binding_id = Some(binding.id);
        item.hostname = binding.host.clone();
        item.challenge_token = Some("token".to_owned());
        item.challenge_value = Some("token.thumbprint".to_owned());
        item.challenge_expires_at = Some(OffsetDateTime::now_utc() + time::Duration::minutes(5));
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([vec![ingress.clone()]])
            .append_query_results([vec![item]])
            .append_query_results([vec![ingress]])
            .append_query_results([vec![binding.clone()]])
            .into_connection();
        let snapshot = snapshot(&db, "eu", "secret").await.unwrap();
        assert!(snapshot.bundles.is_empty());
        assert_eq!(snapshot.challenges.len(), 1);
        assert_eq!(snapshot.challenges[0].hostname, binding.host);
        assert!(
            !db.into_transaction_log()
                .iter()
                .any(|entry| format!("{entry:?}").contains("deployments"))
        );
    }

    #[tokio::test]
    async fn stale_generation_cannot_restore_replaced_issuer_or_challenge() {
        let ingress = ingress_fixture();
        let mut old = certificate_fixture(&ingress);
        old.issuer = "zerossl".to_owned();
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([Vec::<cert::Model>::new()])
            .into_connection();
        assert!(
            super::super::acme::save_current(&db, &old, old.clone().into())
                .await
                .is_err()
        );
        let log = format!("{:?}", db.into_transaction_log());
        assert!(log.contains("generation"));
    }
    #[test]
    fn provider_config_is_encrypted_bound_and_patchable() {
        let id = Uuid::now_v7();
        let value = json!({"api_token":"secret", "zone":"example.org"});
        let sealed = seal_config(id, &value, "key").unwrap();
        assert!(!sealed.to_string().contains("secret"));
        assert_eq!(decrypt("key", id, CONFIG_KEY, &sealed).unwrap(), value);
        assert!(decrypt("key", Uuid::now_v7(), CONFIG_KEY, &sealed).is_err());
        assert_eq!(
            merge_config(
                value,
                &json!({"api_token":"", "zone":null, "contact_email":"a@example.org"})
            )
            .unwrap(),
            json!({"api_token":"secret","contact_email":"a@example.org"})
        );
    }
    #[test]
    fn certificate_name_key_and_signed_expiry_are_validated() {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["site.example.org".to_owned()]).unwrap();
        let bundle = PemBundle {
            certificate_pem: cert.pem(),
            private_key_pem: signing_key.serialize_pem(),
        };
        let validity =
            validate_pem("site.example.org", &bundle, OffsetDateTime::now_utc()).unwrap();
        assert!(validity.expires_at > OffsetDateTime::now_utc());
        assert!(validate_pem("other.example.org", &bundle, OffsetDateTime::now_utc()).is_err());
        assert!(validate_pem("site.example.org", &bundle, validity.expires_at).is_err());
        let other = rcgen::KeyPair::generate().unwrap();
        assert!(
            validate_pem(
                "site.example.org",
                &PemBundle {
                    certificate_pem: bundle.certificate_pem,
                    private_key_pem: other.serialize_pem()
                },
                OffsetDateTime::now_utc()
            )
            .is_err()
        );
    }
}
