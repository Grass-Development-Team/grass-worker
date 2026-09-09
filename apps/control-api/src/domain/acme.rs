//! Automatic issuance with persistent retry state and challenge publication barriers.

use anyhow::{Context, ensure};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, ExternalAccountKey,
    Identifier, LetsEncrypt, NewAccount, NewOrder, OrderStatus, RetryPolicy, ZeroSsl,
};
use sea_orm::{
    ColumnTrait, Condition, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect, Set,
    TransactionTrait, sea_query::Expr,
};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::certificates::{self, ACCOUNT_KEY, BUNDLE_KEY, PemBundle};
use crate::infra::{
    database::entity::{
        managed_certificate as cert, node_ingress_status, project_host_binding, regional_ingress,
    },
    host_provision::DnsProviderHostProvisioner,
};

const LEASE_SECONDS: i64 = 600;
const ATTEMPT_SECONDS: u64 = 480;
const CHALLENGE_SECONDS: i64 = 600;
const RENEW_DAYS: i64 = 30;

fn retry_delay(failures: i32) -> Duration {
    Duration::seconds((300_i64.saturating_mul(1_i64 << failures.clamp(0, 8))).min(86_400))
}

fn due(item: &cert::Model, now: OffsetDateTime) -> bool {
    item.issuer != "manual"
        && !item.lease_until.is_some_and(|until| until > now)
        && !item.retry_at.is_some_and(|until| until > now)
        && (matches!(item.status.as_str(), "pending" | "issuing" | "failed")
            || item.bundle.is_none()
            || (item.auto_renew
                && item
                    .expires_at
                    .is_none_or(|until| until - now <= Duration::days(RENEW_DAYS))))
}

/// Checks that the current configuration still owns this issuance.
async fn current(db: &DatabaseConnection, item: &cert::Model) -> anyhow::Result<cert::Model> {
    cert::Entity::find_by_id(item.id)
        .filter(cert::Column::Generation.eq(item.generation))
        .one(db)
        .await?
        .context("certificate configuration changed during issuance")
}

pub(super) async fn save_current(
    db: &DatabaseConnection,
    item: &cert::Model,
    active: cert::ActiveModel,
) -> anyhow::Result<cert::Model> {
    Ok(cert::Entity::update(active)
        .validate()?
        .filter(cert::Column::Generation.eq(item.generation))
        .exec(db)
        .await?)
}

async fn account(
    db: &DatabaseConnection,
    item: &cert::Model,
    config: &Value,
    secret: &str,
) -> anyhow::Result<Account> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    if let Some(value) = &item.acme_account {
        let credentials: AccountCredentials =
            serde_json::from_value(certificates::decrypt(secret, item.id, ACCOUNT_KEY, value)?)?;
        return Ok(Account::builder()?.from_credentials(credentials).await?);
    }
    let directory = match item.issuer.as_str() {
        "letsencrypt" => LetsEncrypt::Production.url(),
        "zerossl" => ZeroSsl::Production.url(),
        _ => anyhow::bail!("unsupported certificate issuer"),
    };
    // Staging CA endpoints are an operator runtime choice, never tenant-controlled API input.
    let directory =
        std::env::var("GRASS_ACME_DIRECTORY_URL").unwrap_or_else(|_| directory.to_owned());
    let contact = config
        .get("contact_email")
        .and_then(Value::as_str)
        .map(|v| format!("mailto:{v}"));
    let contacts = contact.iter().map(String::as_str).collect::<Vec<_>>();
    let eab = external_account_key(config)?;
    ensure!(
        item.issuer != "zerossl" || eab.is_some(),
        "ZeroSSL requires eab_kid and eab_hmac_key"
    );
    let (account, credentials) = Account::builder()?
        .create(
            &NewAccount {
                contact: &contacts,
                terms_of_service_agreed: true,
                only_return_existing: false,
            },
            directory,
            eab.as_ref(),
        )
        .await?;
    let mut active: cert::ActiveModel = current(db, item).await?.into();
    active.acme_account = Set(Some(certificates::encrypt(
        secret,
        item.id,
        ACCOUNT_KEY,
        &serde_json::to_value(credentials)?,
    )?));
    save_current(db, item, active).await?;
    Ok(account)
}

fn external_account_key(config: &Value) -> anyhow::Result<Option<ExternalAccountKey>> {
    let Some(kid) = config.get("eab_kid").and_then(Value::as_str) else {
        return Ok(None);
    };
    let hmac = config
        .get("eab_hmac_key")
        .and_then(Value::as_str)
        .context("eab_hmac_key is required")?;
    let bytes = URL_SAFE_NO_PAD
        .decode(hmac)
        .or_else(|_| STANDARD.decode(hmac))
        .map_err(|_| anyhow::anyhow!("eab_hmac_key must be base64 encoded"))?;
    Ok(Some(ExternalAccountKey::new(kid.to_owned(), &bytes)))
}

async fn wait_http_ack(
    db: &DatabaseConnection,
    item: &cert::Model,
    ingress: &regional_ingress::Model,
    secret: &str,
) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        let current = current(db, item).await?;
        ensure!(
            certificates::eligible(db, &current, ingress).await?,
            "domain is no longer eligible for certificate issuance"
        );
        let snapshot = certificates::snapshot(db, &ingress.region, secret).await?;
        let ready =
            regional_challenge_acknowledged(db, &ingress.region, &snapshot.challenge_revision)
                .await?;
        if ready {
            return Ok(());
        }
        ensure!(
            tokio::time::Instant::now() < deadline,
            "regional entry nodes did not acknowledge the HTTP challenge within 90 seconds"
        );
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

async fn regional_challenge_acknowledged(
    db: &DatabaseConnection,
    region: &str,
    revision: &str,
) -> anyhow::Result<bool> {
    let now = OffsetDateTime::now_utc();
    let nodes = super::ingress::healthy_serve_nodes(db, region, now).await?;
    if nodes.is_empty() {
        return Ok(false);
    }
    for node in nodes {
        let status = node_ingress_status::Entity::find_by_id(Uuid::parse_str(&node.node_id)?)
            .one(db)
            .await?;
        if !status.is_some_and(|s| {
            s.challenge_revision == revision && now - s.checked_at < Duration::seconds(30)
        }) {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn wait_dns(name: &str, value: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        if super::ingress::verify_dns_record_at(
            &client,
            "https://cloudflare-dns.com/dns-query",
            name,
            "TXT",
            value,
        )
        .await?
            == super::ingress::DnsVerification::Verified
        {
            return Ok(());
        }
        ensure!(
            tokio::time::Instant::now() < deadline,
            "DNS challenge has not propagated within 120 seconds"
        );
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

async fn issue(
    db: &DatabaseConnection,
    item: &cert::Model,
    ingress: &regional_ingress::Model,
    config: &Value,
    secret: &str,
) -> anyhow::Result<PemBundle> {
    let account = account(db, item, config, secret).await?;
    issue_order(db, item, ingress, config, secret, &account).await
}

async fn issue_order(
    db: &DatabaseConnection,
    item: &cert::Model,
    ingress: &regional_ingress::Model,
    config: &Value,
    secret: &str,
    account: &Account,
) -> anyhow::Result<PemBundle> {
    let identifiers = [Identifier::Dns(item.hostname.clone())];
    let mut order = account.new_order(&NewOrder::new(&identifiers)).await?;
    {
        let mut authorizations = order.authorizations();
        while let Some(result) = authorizations.next().await {
            let mut authorization = result?;
            if authorization.status == AuthorizationStatus::Valid {
                continue;
            }
            let challenge_type = if item.challenge_method == "http01" {
                ChallengeType::Http01
            } else {
                ChallengeType::Dns01
            };
            let mut challenge = authorization
                .challenge(challenge_type)
                .context("ACME order does not offer the configured challenge method")?;
            let mut active: cert::ActiveModel = current(db, item).await?.into();
            if item.challenge_method == "http01" {
                active.challenge_token = Set(Some(challenge.token.clone()));
                active.challenge_value =
                    Set(Some(challenge.key_authorization().as_str().to_owned()));
                active.challenge_expires_at = Set(Some(
                    OffsetDateTime::now_utc() + Duration::seconds(CHALLENGE_SECONDS),
                ));
                save_current(db, item, active).await?;
                wait_http_ack(db, item, ingress, secret).await?;
            } else {
                let provider = ingress
                    .dns_challenge_provider
                    .as_deref()
                    .context("DNS challenge provider is not configured")?;
                let zone = config
                    .get("zone")
                    .or_else(|| config.get("domain"))
                    .and_then(Value::as_str)
                    .unwrap_or(&ingress.hostname);
                let source_name = format!("_acme-challenge.{}", item.hostname);
                let name = if item.host_binding_id.is_some() {
                    let name = certificates::delegation_target(item, ingress);
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(10))
                        .build()?;
                    ensure!(
                        super::ingress::verify_dns_record_at(
                            &client,
                            "https://cloudflare-dns.com/dns-query",
                            &source_name,
                            "CNAME",
                            &name
                        )
                        .await?
                            == super::ingress::DnsVerification::Verified,
                        "custom domain must delegate its ACME CNAME to the displayed regional challenge target"
                    );
                    name
                } else {
                    source_name.clone()
                };
                let value = challenge.key_authorization().dns_value();
                // Persist before creating DNS so crash recovery can clean this exact TXT value.
                active.dns_record_name = Set(Some(name.clone()));
                active.dns_record_value = Set(Some(value.clone()));
                active.dns_cleanup = Set(Some(certificates::encrypt(
                    secret,
                    item.id,
                    "dns-cleanup-v1",
                    &json!({"provider":provider,"config":config,"zone":zone}),
                )?));
                save_current(db, item, active).await?;
                DnsProviderHostProvisioner::new()
                    .ensure_txt_record(provider, config, zone, &name, &value)
                    .await
                    .map_err(|_| {
                        anyhow::anyhow!("DNS provider could not create the challenge record")
                    })?;
                wait_dns(&source_name, &value).await?;
            }
            let fresh = current(db, item).await?;
            ensure!(
                certificates::eligible(db, &fresh, ingress).await?,
                "domain is no longer eligible for issuance"
            );
            challenge.set_ready().await?;
        }
    }
    let policy = RetryPolicy::new()
        .initial_delay(std::time::Duration::from_secs(2))
        .timeout(std::time::Duration::from_secs(120));
    ensure!(
        order.poll_ready(&policy).await? == OrderStatus::Ready,
        "ACME order did not become ready"
    );
    let private_key_pem = order.finalize().await?;
    let certificate_pem = order.poll_certificate(&policy).await?;
    Ok(PemBundle {
        certificate_pem,
        private_key_pem,
    })
}

async fn cleanup(db: &DatabaseConnection, item: &cert::Model, secret: &str) -> anyhow::Result<()> {
    let fresh = current(db, item).await?;
    let mut dns_clean = true;
    if let (Some(name), Some(value), Some(cleanup)) = (
        &fresh.dns_record_name,
        &fresh.dns_record_value,
        &fresh.dns_cleanup,
    ) {
        let cleanup = certificates::decrypt(secret, item.id, "dns-cleanup-v1", cleanup)?;
        let provider = cleanup["provider"]
            .as_str()
            .context("stored cleanup provider missing")?;
        let zone = cleanup["zone"]
            .as_str()
            .context("stored cleanup zone missing")?;
        dns_clean = DnsProviderHostProvisioner::new()
            .remove_txt_record(provider, &cleanup["config"], zone, name, value)
            .await
            .is_ok();
    }
    let mut active: cert::ActiveModel = fresh.into();
    active.challenge_token = Set(None);
    active.challenge_value = Set(None);
    active.challenge_expires_at = Set(None);
    if dns_clean {
        active.dns_record_name = Set(None);
        active.dns_record_value = Set(None);
        active.dns_cleanup = Set(None);
    }
    save_current(db, item, active).await?;
    ensure!(dns_clean, "DNS challenge cleanup will be retried");
    Ok(())
}

async fn reconcile_record(
    db: &DatabaseConnection,
    item: &cert::Model,
    ingress: &regional_ingress::Model,
    secret: &str,
) -> anyhow::Result<()> {
    let transaction = db.begin().await?;
    let ingress = regional_ingress::Entity::find_by_id(ingress.id)
        .lock_exclusive()
        .one(&transaction)
        .await?
        .context("regional ingress no longer exists")?;
    let now = OffsetDateTime::now_utc();
    if !due(item, now) || !certificates::eligible(&transaction, item, &ingress).await? {
        return Ok(());
    }
    let lock = cert::Entity::update_many()
        .col_expr(
            cert::Column::LeaseUntil,
            Expr::value(now + Duration::seconds(LEASE_SECONDS)),
        )
        .col_expr(cert::Column::Status, Expr::value("issuing"))
        .filter(cert::Column::Id.eq(item.id))
        .filter(cert::Column::Generation.eq(item.generation))
        .filter(
            Condition::any()
                .add(cert::Column::LeaseUntil.is_null())
                .add(cert::Column::LeaseUntil.lte(now)),
        )
        .exec(&transaction)
        .await?;
    transaction.commit().await?;
    if lock.rows_affected == 0 {
        return Ok(());
    }
    let config = certificates::config(&ingress, secret)?;
    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(ATTEMPT_SECONDS),
        issue(db, item, &ingress, &config, secret),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(anyhow::anyhow!("certificate issuance timed out")),
    };
    let cleanup_result = cleanup(db, item, secret).await;
    let fresh = current(db, item).await?;
    let result = result.and_then(|bundle| {
        certificates::validate_pem(&item.hostname, &bundle, OffsetDateTime::now_utc())
            .map(|validity| (bundle, validity))
    });
    let mut active: cert::ActiveModel = fresh.clone().into();
    active.lease_until = Set(None);
    active.updated_at = Set(OffsetDateTime::now_utc());
    match result {
        Ok((bundle, validity)) => {
            ensure!(
                certificates::eligible(db, &fresh, &ingress).await?,
                "domain became ineligible during issuance"
            );
            active.bundle = Set(Some(certificates::encrypt(
                secret,
                item.id,
                BUNDLE_KEY,
                &serde_json::to_value(bundle)?,
            )?));
            active.revision = Set(validity.revision);
            active.issued_at = Set(Some(validity.issued_at));
            active.expires_at = Set(Some(validity.expires_at));
            active.status = Set("active".to_owned());
            active.error = Set(cleanup_result
                .err()
                .map(|_| "DNS cleanup is pending; it will be retried".to_owned()));
            active.retry_at = Set(None);
            active.failure_count = Set(0);
        }
        Err(_error) => {
            // ACME/provider errors may contain challenge/account material; expose bounded diagnostics.
            active.status = Set("failed".to_owned());
            active.error=Set(Some("Certificate issuance failed; check DNS delegation, regional entry acknowledgements and ACME account configuration. Automatic retry is scheduled.".to_owned()));
            active.retry_at = Set(Some(
                OffsetDateTime::now_utc() + retry_delay(item.failure_count),
            ));
            active.failure_count = Set(item.failure_count.saturating_add(1));
        }
    }
    let updated = save_current(db, item, active).await?;
    certificates::sync_regional_status(db, &updated).await?;
    Ok(())
}

pub async fn sweep(db: &DatabaseConnection, secret: &str) -> anyhow::Result<()> {
    let ingresses = regional_ingress::Entity::find().all(db).await?;
    for ingress in ingresses {
        if sweep_ingress(db, ingress.id, secret).await.is_err() {
            tracing::warn!(operation="control_api.acme.ingress_sweep_failed",ingress_id=%ingress.id,"regional certificate sweep failed; other regions will continue");
        }
    }
    Ok(())
}

async fn sweep_ingress(
    db: &DatabaseConnection,
    ingress_id: Uuid,
    secret: &str,
) -> anyhow::Result<()> {
    let Some(ingress) = certificates::prepare_regional_sweep(db, ingress_id, secret).await? else {
        return Ok(());
    };
    if ingress.enabled && ingress.tls_enabled && ingress.deleted_at.is_none() {
        let bindings = project_host_binding::Entity::find()
            .filter(project_host_binding::Column::Region.eq(&ingress.region))
            .filter(project_host_binding::Column::DeletedAt.is_null())
            .all(db)
            .await?;
        for binding in bindings
            .iter()
            .filter(|b| certificates::binding_eligible(b))
        {
            certificates::ensure_record(db, &ingress, Some(binding)).await?;
        }
    }
    let items = cert::Entity::find()
        .filter(cert::Column::IngressId.eq(ingress.id))
        .all(db)
        .await?;
    for item in items {
        if !item
            .lease_until
            .is_some_and(|until| until > OffsetDateTime::now_utc())
        {
            // Recover challenges after process interruption or domain/ingress removal.
            if (item.dns_record_name.is_some() || item.challenge_token.is_some())
                && cleanup(db, &item, secret).await.is_err()
            {
                continue;
            }
        }
        if let Err(_error) = reconcile_record(db, &item, &ingress, secret).await {
            tracing::warn!(operation="control_api.acme.reconcile_failed",certificate_id=%item.id,"certificate reconciliation failed; state retained for retry");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn malformed_legacy_bundle_does_not_block_other_regions() {
        let mut first = certificates::tests::ingress_fixture();
        first.certificate_issuer = "manual".to_owned();
        first.certificate_auto_renew = false;
        first.dns_challenge_config =
            certificates::seal_config(first.id, &json!({}), "secret").unwrap();
        first.certificate_bundle = Some(json!({"invalid_old_envelope":true}));
        let mut second = certificates::tests::ingress_fixture();
        second.region = "us".to_owned();
        second.hostname = "us.example.org".to_owned();
        second.certificate_issuer = "manual".to_owned();
        second.certificate_auto_renew = false;
        second.dns_challenge_config =
            certificates::seal_config(second.id, &json!({}), "secret").unwrap();
        let manual = |ingress: &regional_ingress::Model| {
            let mut item = certificates::tests::certificate_fixture(ingress);
            item.issuer = "manual".to_owned();
            item.auto_renew = false;
            item
        };
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([vec![first.clone(), second.clone()], vec![first.clone()]])
            .append_query_results([vec![manual(&first)]])
            .append_query_results([Vec::<project_host_binding::Model>::new()])
            .append_query_results([Vec::<cert::Model>::new()])
            .append_query_results([vec![second.clone()]])
            .append_query_results([vec![manual(&second)]])
            .append_query_results([Vec::<project_host_binding::Model>::new()])
            .append_query_results([Vec::<cert::Model>::new()])
            .into_connection();
        sweep(&db, "secret").await.unwrap();
        let log = db.into_transaction_log();
        assert_eq!(
            log.iter()
                .flat_map(|t| t.statements())
                .filter(|s| s.sql.contains("FROM \"project_host_bindings\""))
                .count(),
            2
        );
        assert!(
            !log.iter()
                .flat_map(|t| t.statements())
                .any(|s| s.sql.starts_with("UPDATE "))
        );
    }

    #[tokio::test]
    async fn malformed_legacy_provider_config_is_isolated_to_its_region() {
        let mut broken = certificates::tests::ingress_fixture();
        broken.dns_challenge_config = json!(false);
        let mut healthy = certificates::tests::ingress_fixture();
        healthy.enabled = false;
        healthy.dns_challenge_config =
            certificates::seal_config(healthy.id, &json!({}), "secret").unwrap();
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([
                vec![broken.clone(), healthy.clone()],
                vec![broken],
                vec![healthy.clone()],
            ])
            .append_query_results([Vec::<cert::Model>::new()])
            .into_connection();
        sweep(&db, "secret").await.unwrap();
        let healthy_id: sea_orm::Value = healthy.id.into();
        assert!(
            db.into_transaction_log()
                .iter()
                .flat_map(|t| t.statements())
                .any(|s| s.sql.contains("FROM \"managed_certificates\"")
                    && s.values
                        .as_ref()
                        .is_some_and(|values| values.0.contains(&healthy_id)))
        );
    }
    #[tokio::test]
    async fn http_validation_waits_for_every_eligible_entry_revision() {
        use crate::infra::database::entity::regional_ingress_health;
        let ingress = certificates::tests::ingress_fixture();
        let first = super::super::ingress::tests::node_fixture();
        let second = super::super::ingress::tests::node_fixture();
        let now = OffsetDateTime::now_utc();
        let revision = "a".repeat(64);
        for second_revision in ["b".repeat(64), revision.clone()] {
            let nodes = vec![first.clone(), second.clone()];
            let statuses = vec![
                node_ingress_status::Model {
                    node_id: first.id,
                    certificates: json!([]),
                    challenge_revision: revision.clone(),
                    tls_ready: true,
                    checked_at: now,
                },
                node_ingress_status::Model {
                    node_id: second.id,
                    certificates: json!([]),
                    challenge_revision: second_revision.clone(),
                    tls_ready: true,
                    checked_at: now,
                },
            ];
            let health = nodes
                .iter()
                .map(|n| regional_ingress_health::Model {
                    ingress_id: ingress.id,
                    node_id: n.id,
                    status: "healthy".to_owned(),
                    checked_at: Some(now),
                    latency_ms: Some(1),
                    error: None,
                })
                .collect::<Vec<_>>();
            let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
                .append_query_results([vec![ingress.clone()]])
                .append_query_results([health])
                .append_query_results([nodes])
                .append_query_results([
                    statuses.clone(),
                    vec![statuses[0].clone()],
                    vec![statuses[1].clone()],
                ])
                .into_connection();
            assert_eq!(
                regional_challenge_acknowledged(&db, "eu", &revision)
                    .await
                    .unwrap(),
                second_revision == revision
            );
        }
    }
    #[derive(Clone)]
    struct MockCa {
        origin: String,
        certificate: std::sync::Arc<std::sync::Mutex<Option<String>>>,
        accounts: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        issued: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    async fn mock_ca(
        axum::extract::State(state): axum::extract::State<MockCa>,
        uri: axum::http::Uri,
        body: axum::body::Bytes,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        use std::sync::atomic::Ordering;
        let payload = if body.is_empty() {
            json!({})
        } else {
            let body: Value = serde_json::from_slice(&body).unwrap();
            let encoded = body["payload"].as_str().unwrap();
            if encoded.is_empty() {
                json!({})
            } else {
                serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).unwrap()).unwrap()
            }
        };
        let order = || json!({"status":if state.issued.load(Ordering::SeqCst){"valid"}else{"ready"},"authorizations":[format!("{}/authorization",state.origin)],"finalize":format!("{}/finalize",state.origin),"certificate":if state.issued.load(Ordering::SeqCst){Some(format!("{}/certificate",state.origin))}else{None}});
        let mut response=match uri.path(){
            "/directory"=>axum::Json(json!({"newNonce":format!("{}/nonce",state.origin),"newAccount":format!("{}/account",state.origin),"newOrder":format!("{}/new-order",state.origin)})).into_response(),
            "/nonce"=>axum::http::StatusCode::OK.into_response(),
            "/account"=>{state.accounts.fetch_add(1,Ordering::SeqCst);axum::Json(json!({"status":"valid"})).into_response()},
            "/new-order"=>{assert_eq!(payload["identifiers"][0]["value"],"site.example.org");axum::Json(order()).into_response()},
            "/authorization"=>axum::Json(json!({"status":"valid","identifier":{"type":"dns","value":"site.example.org"},"challenges":[]})).into_response(),
            "/finalize"=>{
                let der=URL_SAFE_NO_PAD.decode(payload["csr"].as_str().unwrap()).unwrap();
                let mut csr=rcgen::CertificateSigningRequestParams::from_der(&der.into()).unwrap();
                csr.params.not_before=OffsetDateTime::now_utc()-Duration::minutes(1);csr.params.not_after=OffsetDateTime::now_utc()+Duration::days(14);
                let issuer=rcgen::Issuer::new(rcgen::CertificateParams::default(),rcgen::KeyPair::generate().unwrap());
                *state.certificate.lock().unwrap()=Some(csr.signed_by(&issuer).unwrap().pem());state.issued.store(true,Ordering::SeqCst);axum::Json(order()).into_response()
            },
            "/order"=>axum::Json(order()).into_response(),
            "/certificate"=>state.certificate.lock().unwrap().clone().unwrap().into_response(),
            _=>axum::http::StatusCode::NOT_FOUND.into_response(),
        };
        response
            .headers_mut()
            .insert("replay-nonce", "bW9jay1ub25jZQ".parse().unwrap());
        if uri.path() == "/account" {
            response.headers_mut().insert(
                "location",
                format!("{}/account/1", state.origin).parse().unwrap(),
            );
        }
        if uri.path() == "/new-order" {
            response.headers_mut().insert(
                "location",
                format!("{}/order", state.origin).parse().unwrap(),
            );
        }
        response
    }

    #[tokio::test]
    async fn mock_acme_issues_a_key_matched_certificate_and_reuses_encrypted_account() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let state = MockCa {
            origin: origin.clone(),
            certificate: Default::default(),
            accounts: Default::default(),
            issued: Default::default(),
        };
        let app = axum::Router::new()
            .fallback(mock_ca)
            .with_state(state.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client =
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .build_http::<instant_acme::BodyWrapper<bytes::Bytes>>();
        let (_, credentials) = Account::builder_with_http(Box::new(client.clone()))
            .create(
                &NewAccount {
                    contact: &[],
                    terms_of_service_agreed: true,
                    only_return_existing: false,
                },
                format!("{origin}/directory"),
                None,
            )
            .await
            .unwrap();
        let ingress = certificates::tests::ingress_fixture();
        let mut item = certificates::tests::certificate_fixture(&ingress);
        item.hostname = "site.example.org".to_owned();
        item.acme_account = Some(
            certificates::encrypt(
                "test-key",
                item.id,
                ACCOUNT_KEY,
                &serde_json::to_value(credentials).unwrap(),
            )
            .unwrap(),
        );
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres).into_connection();
        let credentials = serde_json::from_value(
            certificates::decrypt(
                "test-key",
                item.id,
                ACCOUNT_KEY,
                item.acme_account.as_ref().unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let account = Account::builder_with_http(Box::new(client))
            .from_credentials(credentials)
            .await
            .unwrap();
        let bundle = issue_order(&db, &item, &ingress, &json!({}), "test-key", &account)
            .await
            .unwrap();
        let validity =
            certificates::validate_pem(&item.hostname, &bundle, OffsetDateTime::now_utc()).unwrap();
        assert!((validity.expires_at - OffsetDateTime::now_utc()).whole_days() <= 14);
        assert_eq!(state.accounts.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(state.issued.load(std::sync::atomic::Ordering::SeqCst));
        server.abort();
    }
    #[test]
    fn renewal_respects_expiry_manual_mode_backoff_and_active_lease() {
        let now = OffsetDateTime::now_utc();
        let ingress = certificates::tests::ingress_fixture();
        let mut item = certificates::tests::certificate_fixture(&ingress);
        assert!(due(&item, now));
        item.status = "active".to_owned();
        item.bundle = Some(json!({}));
        item.expires_at = Some(now + Duration::days(40));
        assert!(!due(&item, now));
        item.expires_at = Some(now + Duration::days(20));
        assert!(due(&item, now));
        item.auto_renew = false;
        assert!(!due(&item, now));
        item.status = "pending".to_owned();
        assert!(due(&item, now));
        item.retry_at = Some(now + Duration::seconds(1));
        assert!(!due(&item, now));
        item.retry_at = None;
        item.lease_until = Some(now + Duration::seconds(1));
        assert!(!due(&item, now));
        item.lease_until = None;
        item.issuer = "manual".to_owned();
        assert!(!due(&item, now));
    }
    #[test]
    fn interrupted_or_failed_forced_renewal_retries_with_auto_renew_disabled() {
        let now = OffsetDateTime::now_utc();
        let ingress = certificates::tests::ingress_fixture();
        let mut item = certificates::tests::certificate_fixture(&ingress);
        item.auto_renew = false;
        item.bundle = Some(json!({}));
        item.expires_at = Some(now + Duration::days(60));
        item.status = "issuing".to_owned();
        item.lease_until = Some(now - Duration::seconds(1));
        assert!(due(&item, now));
        item.status = "failed".to_owned();
        item.retry_at = Some(now + Duration::seconds(1));
        assert!(!due(&item, now));
        assert!(due(&item, now + Duration::seconds(2)));
    }
    #[test]
    fn retry_delay_is_bounded_and_zero_ssl_requires_complete_eab() {
        assert_eq!(retry_delay(0), Duration::minutes(5));
        assert_eq!(retry_delay(20), Duration::seconds(76_800));
        assert!(external_account_key(&json!({"eab_kid":"id"})).is_err());
        assert!(
            external_account_key(&json!({"eab_kid":"id","eab_hmac_key":"c2VjcmV0"}))
                .unwrap()
                .is_some()
        );
    }
}
