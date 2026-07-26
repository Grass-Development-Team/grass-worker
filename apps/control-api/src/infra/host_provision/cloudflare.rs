//! Cloudflare DNS client for `dns_provider` host sources.
//!
//! The host source `config` (write-only through the admin API) carries the
//! zone credentials and the record template; every provisioned host becomes
//! one DNS record `{host} -> record_value`. Provisioning is idempotent: an
//! existing record with the desired shape is accepted, a diverging one is
//! updated in place.

use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;

use super::HostProvisionError;
use crate::infra::database::entity::host_source;

pub const PROVIDER_NAME: &str = "cloudflare";
const DEFAULT_BASE_URL: &str = "https://api.cloudflare.com/client/v4";
/// "Record already exists" family of Cloudflare error codes.
const RECORD_EXISTS_CODES: [i64; 2] = [81_057, 81_053];

/// Parsed view of a Cloudflare host source `config` object.
#[derive(Debug)]
pub struct CloudflareConfig {
    pub api_token: String,
    pub zone_id: String,
    /// `A`, `AAAA`, or `CNAME`.
    pub record_type: String,
    /// Record content: the node IP for `A`/`AAAA`, a hostname for `CNAME`.
    pub record_value: String,
    pub proxied: bool,
    /// Seconds; `1` means Cloudflare-automatic.
    pub ttl: u32,
}

impl CloudflareConfig {
    pub fn from_source(source: &host_source::Model) -> Result<Self, String> {
        Self::from_json(&source.config)
    }

    pub fn from_json(config: &serde_json::Value) -> Result<Self, String> {
        let object = config
            .as_object()
            .ok_or_else(|| "config must be a JSON object".to_owned())?;
        let string_field = |key: &str| -> Result<String, String> {
            object
                .get(key)
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("config.{key} is required"))
        };

        let api_token = string_field("api_token")?;
        let zone_id = string_field("zone_id")?;
        let record_type = string_field("record_type")?.to_ascii_uppercase();
        if !matches!(record_type.as_str(), "A" | "AAAA" | "CNAME") {
            return Err("config.record_type must be A, AAAA, or CNAME".to_owned());
        }
        let record_value = string_field("record_value")?;
        let proxied = match object.get("proxied") {
            None | Some(serde_json::Value::Null) => false,
            Some(serde_json::Value::Bool(value)) => *value,
            Some(_) => return Err("config.proxied must be a boolean".to_owned()),
        };
        let ttl = match object.get("ttl") {
            None | Some(serde_json::Value::Null) => 1,
            Some(value) => {
                let seconds = value
                    .as_u64()
                    .filter(|seconds| *seconds == 1 || (60..=86_400).contains(seconds))
                    .ok_or_else(|| {
                        "config.ttl must be 1 (automatic) or 60-86400 seconds".to_owned()
                    })?;
                u32::try_from(seconds).expect("ttl bounds fit u32")
            }
        };

        Ok(Self {
            api_token,
            zone_id,
            record_type,
            record_value,
            proxied,
            ttl,
        })
    }
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("reqwest client construction cannot fail with static options")
    })
}

#[derive(Deserialize)]
struct ApiEnvelope<T> {
    success: bool,
    #[serde(default)]
    errors: Vec<ApiErrorEntry>,
    result: Option<T>,
}

#[derive(Deserialize)]
struct ApiErrorEntry {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
pub struct DnsRecord {
    pub id: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub proxied: bool,
}

#[derive(Debug)]
pub struct EnsuredRecord {
    pub id: String,
    pub updated: bool,
}

enum CreateOutcome {
    Created(DnsRecord),
    AlreadyExists,
}

fn request_error(error: reqwest::Error) -> HostProvisionError {
    // `without_url` keeps zone ids out of user-facing failure reasons; the
    // token only ever travels in a header and cannot appear here.
    HostProvisionError::Provider(format!(
        "cloudflare request failed: {}",
        error.without_url()
    ))
}

fn api_error(operation: &str, errors: &[ApiErrorEntry]) -> HostProvisionError {
    let details = if errors.is_empty() {
        "unknown error".to_owned()
    } else {
        errors
            .iter()
            .map(|entry| format!("{} (code {})", entry.message, entry.code))
            .collect::<Vec<_>>()
            .join("; ")
    };
    HostProvisionError::Provider(format!("cloudflare could not {operation}: {details}"))
}

fn record_body(config: &CloudflareConfig, host: &str) -> serde_json::Value {
    json!({
        "type": config.record_type,
        "name": host,
        "content": config.record_value,
        "proxied": config.proxied,
        "ttl": config.ttl,
    })
}

/// Thin Cloudflare v4 API client scoped to DNS record management.
pub struct CloudflareDns {
    base_url: String,
}

impl Default for CloudflareDns {
    fn default() -> Self {
        Self::new()
    }
}

impl CloudflareDns {
    pub fn new() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }

    #[cfg(test)]
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    async fn parse<T: DeserializeOwned>(
        response: reqwest::Response,
    ) -> Result<ApiEnvelope<T>, HostProvisionError> {
        response
            .json::<ApiEnvelope<T>>()
            .await
            .map_err(request_error)
    }

    /// Create the record for `host`, or reconcile with an existing one.
    pub async fn ensure_record(
        &self,
        config: &CloudflareConfig,
        host: &str,
    ) -> Result<EnsuredRecord, HostProvisionError> {
        match self.create_record(config, host).await? {
            CreateOutcome::Created(record) => Ok(EnsuredRecord {
                id: record.id,
                updated: false,
            }),
            CreateOutcome::AlreadyExists => {
                let existing = self.find_record(config, host).await?.ok_or_else(|| {
                    HostProvisionError::Provider(format!(
                        "cloudflare reports an existing record for {host}, but none was found in the zone"
                    ))
                })?;
                let matches = existing.record_type == config.record_type
                    && existing.content == config.record_value
                    && existing.proxied == config.proxied;
                if matches {
                    Ok(EnsuredRecord {
                        id: existing.id,
                        updated: false,
                    })
                } else {
                    let updated = self.update_record(config, &existing.id, host).await?;
                    Ok(EnsuredRecord {
                        id: updated.id,
                        updated: true,
                    })
                }
            }
        }
    }

    /// Delete the record for `host` if one exists. Returns the deleted
    /// record id, or `None` when the zone has no record for the host.
    pub async fn remove_record(
        &self,
        config: &CloudflareConfig,
        host: &str,
    ) -> Result<Option<String>, HostProvisionError> {
        let Some(existing) = self.find_record(config, host).await? else {
            return Ok(None);
        };
        let response = http_client()
            .delete(format!(
                "{}/zones/{}/dns_records/{}",
                self.base_url, config.zone_id, existing.id
            ))
            .bearer_auth(&config.api_token)
            .send()
            .await
            .map_err(request_error)?;
        let body: ApiEnvelope<serde_json::Value> = Self::parse(response).await?;
        if body.success {
            Ok(Some(existing.id))
        } else {
            Err(api_error("delete the DNS record", &body.errors))
        }
    }

    async fn create_record(
        &self,
        config: &CloudflareConfig,
        host: &str,
    ) -> Result<CreateOutcome, HostProvisionError> {
        let response = http_client()
            .post(format!(
                "{}/zones/{}/dns_records",
                self.base_url, config.zone_id
            ))
            .bearer_auth(&config.api_token)
            .json(&record_body(config, host))
            .send()
            .await
            .map_err(request_error)?;
        let body: ApiEnvelope<DnsRecord> = Self::parse(response).await?;
        if body.success {
            let record = body.result.ok_or_else(|| {
                HostProvisionError::Provider(
                    "cloudflare returned success without a record".to_owned(),
                )
            })?;
            return Ok(CreateOutcome::Created(record));
        }
        if body
            .errors
            .iter()
            .any(|entry| RECORD_EXISTS_CODES.contains(&entry.code))
        {
            return Ok(CreateOutcome::AlreadyExists);
        }
        Err(api_error("create the DNS record", &body.errors))
    }

    async fn find_record(
        &self,
        config: &CloudflareConfig,
        host: &str,
    ) -> Result<Option<DnsRecord>, HostProvisionError> {
        let response = http_client()
            .get(format!(
                "{}/zones/{}/dns_records",
                self.base_url, config.zone_id
            ))
            .query(&[("name", host)])
            .bearer_auth(&config.api_token)
            .send()
            .await
            .map_err(request_error)?;
        let body: ApiEnvelope<Vec<DnsRecord>> = Self::parse(response).await?;
        if !body.success {
            return Err(api_error("list DNS records", &body.errors));
        }
        let records = body.result.unwrap_or_default();
        let mut matching: Vec<DnsRecord> = records
            .into_iter()
            .filter(|record| record.name == host)
            .collect();
        // Prefer a record of the configured type when several names match
        // (e.g. an A and a TXT record on the same name).
        if let Some(index) = matching
            .iter()
            .position(|record| record.record_type == config.record_type)
        {
            return Ok(Some(matching.swap_remove(index)));
        }
        Ok(matching.into_iter().next())
    }

    async fn update_record(
        &self,
        config: &CloudflareConfig,
        record_id: &str,
        host: &str,
    ) -> Result<DnsRecord, HostProvisionError> {
        let response = http_client()
            .put(format!(
                "{}/zones/{}/dns_records/{}",
                self.base_url, config.zone_id, record_id
            ))
            .bearer_auth(&config.api_token)
            .json(&record_body(config, host))
            .send()
            .await
            .map_err(request_error)?;
        let body: ApiEnvelope<DnsRecord> = Self::parse(response).await?;
        if body.success {
            body.result.ok_or_else(|| {
                HostProvisionError::Provider(
                    "cloudflare returned success without a record".to_owned(),
                )
            })
        } else {
            Err(api_error("update the DNS record", &body.errors))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::extract::State;
    use axum::routing::{delete, get, post, put};
    use axum::{Json, Router};

    use super::*;

    fn config() -> CloudflareConfig {
        CloudflareConfig {
            api_token: "token".to_owned(),
            zone_id: "zone1".to_owned(),
            record_type: "A".to_owned(),
            record_value: "203.0.113.7".to_owned(),
            proxied: false,
            ttl: 1,
        }
    }

    fn envelope(result: serde_json::Value) -> serde_json::Value {
        json!({ "success": true, "errors": [], "result": result })
    }

    fn failure(code: i64, message: &str) -> serde_json::Value {
        json!({ "success": false, "errors": [{ "code": code, "message": message }], "result": null })
    }

    async fn spawn(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        base
    }

    #[test]
    fn config_requires_core_fields() {
        let error = CloudflareConfig::from_json(&json!({})).unwrap_err();
        assert!(error.contains("api_token"));

        let error = CloudflareConfig::from_json(&json!({
            "api_token": "t", "zone_id": "z", "record_type": "TXT", "record_value": "v"
        }))
        .unwrap_err();
        assert!(error.contains("record_type"));

        let parsed = CloudflareConfig::from_json(&json!({
            "api_token": "t", "zone_id": "z", "record_type": "cname",
            "record_value": "node.example.com", "proxied": true, "ttl": 300
        }))
        .unwrap();
        assert_eq!(parsed.record_type, "CNAME");
        assert!(parsed.proxied);
        assert_eq!(parsed.ttl, 300);
    }

    #[tokio::test]
    async fn ensure_record_creates_a_fresh_record() {
        let router = Router::new().route(
            "/zones/zone1/dns_records",
            post(|| async {
                Json(envelope(json!({
                    "id": "rec-1", "type": "A", "name": "demo.grass.test",
                    "content": "203.0.113.7", "proxied": false
                })))
            }),
        );
        let base = spawn(router).await;

        let ensured = CloudflareDns::with_base_url(base)
            .ensure_record(&config(), "demo.grass.test")
            .await
            .unwrap();
        assert_eq!(ensured.id, "rec-1");
        assert!(!ensured.updated);
    }

    #[tokio::test]
    async fn ensure_record_reconciles_an_existing_record() {
        let updates: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let router = Router::new()
            .route(
                "/zones/zone1/dns_records",
                post(|| async { Json(failure(81_057, "Record already exists.")) }).get(|| async {
                    Json(envelope(json!([{
                        "id": "rec-9", "type": "A", "name": "demo.grass.test",
                        "content": "198.51.100.1", "proxied": false
                    }])))
                }),
            )
            .route(
                "/zones/zone1/dns_records/{id}",
                put(
                    |State(updates): State<Arc<Mutex<Vec<String>>>>,
                     axum::extract::Path(id): axum::extract::Path<String>| async move {
                        updates.lock().unwrap().push(id);
                        Json(envelope(json!({
                            "id": "rec-9", "type": "A", "name": "demo.grass.test",
                            "content": "203.0.113.7", "proxied": false
                        })))
                    },
                ),
            )
            .with_state(updates.clone());
        let base = spawn(router).await;

        let ensured = CloudflareDns::with_base_url(base)
            .ensure_record(&config(), "demo.grass.test")
            .await
            .unwrap();
        assert_eq!(ensured.id, "rec-9");
        assert!(ensured.updated);
        assert_eq!(updates.lock().unwrap().as_slice(), ["rec-9"]);
    }

    #[tokio::test]
    async fn ensure_record_surfaces_api_errors() {
        let router = Router::new().route(
            "/zones/zone1/dns_records",
            post(|| async { Json(failure(9109, "Invalid access token")) }),
        );
        let base = spawn(router).await;

        let error = CloudflareDns::with_base_url(base)
            .ensure_record(&config(), "demo.grass.test")
            .await
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("Invalid access token"), "{message}");
        assert!(message.contains("9109"), "{message}");
    }

    #[tokio::test]
    async fn remove_record_deletes_when_present_and_ignores_absent_hosts() {
        let deletions: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let router = Router::new()
            .route(
                "/zones/zone1/dns_records",
                get(
                    |axum::extract::Query(query): axum::extract::Query<
                        std::collections::HashMap<String, String>,
                    >| async move {
                        if query.get("name").map(String::as_str) == Some("demo.grass.test") {
                            Json(envelope(json!([{
                                "id": "rec-2", "type": "A", "name": "demo.grass.test",
                                "content": "203.0.113.7", "proxied": false
                            }])))
                        } else {
                            Json(envelope(json!([])))
                        }
                    },
                ),
            )
            .route(
                "/zones/zone1/dns_records/{id}",
                delete(
                    |State(deletions): State<Arc<Mutex<Vec<String>>>>,
                     axum::extract::Path(id): axum::extract::Path<String>| async move {
                        deletions.lock().unwrap().push(id.clone());
                        Json(envelope(json!({ "id": id })))
                    },
                ),
            )
            .with_state(deletions.clone());
        let base = spawn(router).await;
        let dns = CloudflareDns::with_base_url(base);

        let removed = dns
            .remove_record(&config(), "demo.grass.test")
            .await
            .unwrap();
        assert_eq!(removed.as_deref(), Some("rec-2"));
        assert_eq!(deletions.lock().unwrap().as_slice(), ["rec-2"]);

        let removed = dns
            .remove_record(&config(), "gone.grass.test")
            .await
            .unwrap();
        assert_eq!(removed, None);
    }
}
