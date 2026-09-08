//! Tencent Cloud DNSPod DNS client.
//!
//! DNSPod exposes its DNS API through Tencent Cloud API 3.0. Requests are
//! authenticated with TC3-HMAC-SHA256 and sent as JSON. A host source stores
//! the secret pair write-only in its config; API responses and errors never
//! include either secret.

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::Url;
use ring::hmac;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use super::HostProvisionError;
use crate::infra::database::entity::host_source;

pub const PROVIDER_NAME: &str = "dnspod";
const DEFAULT_BASE_URL: &str = "https://dnspod.tencentcloudapi.com";
const API_VERSION: &str = "2021-03-23";

/// Parsed view of a DNSPod host source config object.
#[derive(Debug, Clone)]
pub struct DnsPodConfig {
    pub secret_id: String,
    pub secret_key: String,
    pub domain: String,
    pub record_type: String,
    pub record_value: String,
    pub record_line: String,
    pub ttl: u32,
    endpoint: String,
}

impl DnsPodConfig {
    pub fn from_source(source: &host_source::Model) -> Result<Self, String> {
        Self::from_json(&source.base_domain, &source.config)
    }

    #[allow(dead_code)]
    pub fn for_txt(&self, value: &str) -> Self {
        let mut config = self.clone();
        config.record_type = "TXT".to_owned();
        config.record_value = value.to_owned();
        config
    }

    pub fn from_json(base_domain: &str, config: &Value) -> Result<Self, String> {
        let object = config
            .as_object()
            .ok_or_else(|| "config must be a JSON object".to_owned())?;
        let string_field = |key: &str| -> Result<String, String> {
            object
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("config.{key} is required"))
        };

        let secret_id = string_field("secret_id")?;
        let secret_key = string_field("secret_key")?;
        let domain = object
            .get("domain")
            .and_then(Value::as_str)
            .unwrap_or(base_domain)
            .trim()
            .trim_end_matches('.')
            .to_ascii_lowercase();
        if domain.is_empty() {
            return Err("config.domain is required".to_owned());
        }
        let record_type = string_field("record_type")?.to_ascii_uppercase();
        if !matches!(record_type.as_str(), "A" | "AAAA" | "CNAME") {
            return Err("config.record_type must be A, AAAA, or CNAME".to_owned());
        }
        let record_value = string_field("record_value")?;
        let record_line = object
            .get("record_line")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("\u{9ed8}\u{8ba4}")
            .to_owned();
        let ttl = parse_ttl(object.get("ttl"), 600)?;
        let endpoint = object
            .get("api_endpoint")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/')
            .to_owned();
        let endpoint_url = Url::parse(&endpoint).map_err(|_| "config.api_endpoint is invalid")?;
        if !matches!(endpoint_url.scheme(), "http" | "https") || endpoint_url.host_str().is_none() {
            return Err("config.api_endpoint must be an http(s) URL".to_owned());
        }

        Ok(Self {
            secret_id,
            secret_key,
            domain,
            record_type,
            record_value,
            record_line,
            ttl,
            endpoint,
        })
    }
}

fn parse_ttl(value: Option<&Value>, default: u32) -> Result<u32, String> {
    match value {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value
            .as_u64()
            .filter(|ttl| (1..=86_400).contains(ttl))
            .and_then(|ttl| u32::try_from(ttl).ok())
            .ok_or_else(|| "config.ttl must be an integer from 1-86400 seconds".to_owned()),
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

#[derive(Debug, Clone)]
struct DnsRecord {
    id: String,
    name: String,
    record_type: String,
    value: String,
    line: String,
    ttl: u32,
}

#[derive(Debug)]
pub struct EnsuredRecord {
    pub id: String,
    pub updated: bool,
}

fn request_error(error: reqwest::Error) -> HostProvisionError {
    HostProvisionError::Provider(format!("dnspod request failed: {}", error.without_url()))
}

fn endpoint_host(endpoint: &str) -> Result<String, HostProvisionError> {
    let url = Url::parse(endpoint)
        .map_err(|_| HostProvisionError::Provider("dnspod endpoint URL is invalid".to_owned()))?;
    let host = url
        .host_str()
        .ok_or_else(|| HostProvisionError::Provider("dnspod endpoint has no host".to_owned()))?;
    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    })
}

fn sha256_hex(input: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(input.as_ref()))
}

fn hmac_bytes(key: &[u8], message: &str) -> Vec<u8> {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    hmac::sign(&key, message.as_bytes()).as_ref().to_vec()
}

fn tc3_date(timestamp: i64) -> String {
    let date = OffsetDateTime::from_unix_timestamp(timestamp).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

fn tc3_authorization(
    secret_id: &str,
    secret_key: &str,
    service: &str,
    host: &str,
    _action: &str,
    body: &str,
    timestamp: i64,
) -> String {
    let date = tc3_date(timestamp);
    let timestamp = timestamp.to_string();
    let canonical_headers = format!("content-type:application/json; charset=utf-8\nhost:{host}\n");
    let signed_headers = "content-type;host";
    let canonical_request = format!(
        "POST\n/\n\n{canonical_headers}\n{signed_headers}\n{}",
        sha256_hex(body),
    );
    let credential_scope = format!("{date}/{service}/tc3_request");
    let string_to_sign = format!(
        "TC3-HMAC-SHA256\n{timestamp}\n{credential_scope}\n{}",
        sha256_hex(canonical_request),
    );
    let secret_date = hmac_bytes(format!("TC3{secret_key}").as_bytes(), &date);
    let secret_service = hmac_bytes(&secret_date, service);
    let secret_signing = hmac_bytes(&secret_service, "tc3_request");
    let signature = hex::encode(hmac_bytes(&secret_signing, &string_to_sign));
    format!(
        "TC3-HMAC-SHA256 Credential={secret_id}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}"
    )
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn tc3_signature_for_test(
    secret_key: &str,
    service: &str,
    _host: &str,
    method: &str,
    uri: &str,
    query: &str,
    canonical_headers: &str,
    signed_headers: &str,
    body: &str,
    timestamp: i64,
) -> String {
    let date = tc3_date(timestamp);
    let canonical_request = format!(
        "{method}\n{uri}\n{query}\n{canonical_headers}\n{signed_headers}\n{}",
        sha256_hex(body),
    );
    let credential_scope = format!("{date}/{service}/tc3_request");
    let string_to_sign = format!(
        "TC3-HMAC-SHA256\n{timestamp}\n{credential_scope}\n{}",
        sha256_hex(canonical_request),
    );
    let secret_date = hmac_bytes(format!("TC3{secret_key}").as_bytes(), &date);
    let secret_service = hmac_bytes(&secret_date, service);
    let secret_signing = hmac_bytes(&secret_service, "tc3_request");
    hex::encode(hmac_bytes(&secret_signing, &string_to_sign))
}

fn api_error(action: &str, response: &Value) -> HostProvisionError {
    let error = response
        .get("Response")
        .and_then(|response| response.get("Error"));
    let code = error
        .and_then(|error| error.get("Code"))
        .and_then(Value::as_str)
        .unwrap_or("UnknownError");
    let message = error
        .and_then(|error| error.get("Message"))
        .and_then(Value::as_str)
        .unwrap_or("unknown error");
    HostProvisionError::Provider(format!("dnspod could not {action}: {message} ({code})"))
}

async fn parse_response(
    response: reqwest::Response,
    action: &str,
) -> Result<Value, HostProvisionError> {
    let status = response.status();
    let value = response.json::<Value>().await.map_err(request_error)?;
    if !status.is_success()
        || value
            .get("Response")
            .and_then(|response| response.get("Error"))
            .is_some()
    {
        return Err(api_error(action, &value));
    }
    Ok(value)
}

fn record_from_value(value: &Value) -> Option<DnsRecord> {
    let id = value.get("RecordId").and_then(|id| {
        id.as_str()
            .map(str::to_owned)
            .or_else(|| id.as_i64().map(|id| id.to_string()))
            .or_else(|| id.as_u64().map(|id| id.to_string()))
    })?;
    Some(DnsRecord {
        id,
        name: value.get("Name")?.as_str()?.to_owned(),
        record_type: value.get("Type")?.as_str()?.to_ascii_uppercase(),
        value: value.get("Value")?.as_str()?.to_owned(),
        line: value
            .get("Line")
            .and_then(Value::as_str)
            .unwrap_or("\u{9ed8}\u{8ba4}")
            .to_owned(),
        ttl: value.get("TTL").and_then(Value::as_u64).unwrap_or(600) as u32,
    })
}

/// Thin DNSPod API client scoped to DNS record management.
#[derive(Debug, Clone)]
pub struct DnsPod {
    base_url: String,
}

impl Default for DnsPod {
    fn default() -> Self {
        Self::new()
    }
}

impl DnsPod {
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

    async fn call(
        &self,
        config: &DnsPodConfig,
        action: &str,
        body: Value,
    ) -> Result<Value, HostProvisionError> {
        let body = serde_json::to_string(&body).map_err(|error| {
            HostProvisionError::Provider(format!("dnspod request serialization failed: {error}"))
        })?;
        let endpoint = if config.endpoint == DEFAULT_BASE_URL {
            self.base_url.clone()
        } else {
            config.endpoint.clone()
        };
        let host = endpoint_host(&endpoint)?;
        let timestamp = OffsetDateTime::now_utc().unix_timestamp();
        let authorization = tc3_authorization(
            &config.secret_id,
            &config.secret_key,
            PROVIDER_NAME,
            &host,
            action,
            &body,
            timestamp,
        );
        let response = http_client()
            .post(endpoint)
            .header("content-type", "application/json; charset=utf-8")
            .header("host", &host)
            .header("x-tc-action", action)
            .header("x-tc-version", API_VERSION)
            .header("x-tc-timestamp", timestamp.to_string())
            .header("authorization", authorization)
            .body(body)
            .send()
            .await
            .map_err(request_error)?;
        parse_response(response, action).await
    }

    async fn list_records(
        &self,
        config: &DnsPodConfig,
        host: &str,
    ) -> Result<Vec<DnsRecord>, HostProvisionError> {
        let subdomain = subdomain(&config.domain, host).map_err(HostProvisionError::Provider)?;
        let response = self
            .call(
                config,
                "DescribeRecordList",
                json!({
                    "Domain": config.domain,
                    "Subdomain": subdomain,
                    "RecordType": config.record_type,
                    "RecordLine": config.record_line,
                    "Limit": 100,
                }),
            )
            .await?;
        Ok(response
            .get("Response")
            .and_then(|response| response.get("RecordList"))
            .and_then(Value::as_array)
            .map(|records| records.iter().filter_map(record_from_value).collect())
            .unwrap_or_default())
    }

    async fn create_record(
        &self,
        config: &DnsPodConfig,
        host: &str,
    ) -> Result<String, HostProvisionError> {
        let subdomain = subdomain(&config.domain, host).map_err(HostProvisionError::Provider)?;
        let response = self
            .call(
                config,
                "CreateRecord",
                json!({
                    "Domain": config.domain,
                    "SubDomain": subdomain,
                    "RecordType": config.record_type,
                    "RecordLine": config.record_line,
                    "Value": config.record_value,
                    "TTL": config.ttl,
                }),
            )
            .await?;
        response
            .get("Response")
            .and_then(|response| response.get("RecordId"))
            .and_then(|id| {
                id.as_str()
                    .map(str::to_owned)
                    .or_else(|| id.as_i64().map(|id| id.to_string()))
                    .or_else(|| id.as_u64().map(|id| id.to_string()))
            })
            .ok_or_else(|| {
                HostProvisionError::Provider(
                    "dnspod returned success without a record id".to_owned(),
                )
            })
    }

    async fn update_record(
        &self,
        config: &DnsPodConfig,
        host: &str,
        record: &DnsRecord,
    ) -> Result<(), HostProvisionError> {
        let subdomain = subdomain(&config.domain, host).map_err(HostProvisionError::Provider)?;
        let record_id = record.id.parse::<u64>().map_err(|_| {
            HostProvisionError::Provider("dnspod returned an invalid record id".to_owned())
        })?;
        self.call(
            config,
            "ModifyRecord",
            json!({
                "Domain": config.domain,
                "RecordId": record_id,
                "SubDomain": subdomain,
                "RecordType": config.record_type,
                "RecordLine": config.record_line,
                "Value": config.record_value,
                "TTL": config.ttl,
            }),
        )
        .await
        .map(|_| ())
    }

    #[allow(dead_code)]
    pub async fn ensure_txt_record(
        &self,
        config: &DnsPodConfig,
        name: &str,
        value: &str,
    ) -> Result<EnsuredRecord, HostProvisionError> {
        self.ensure_record(&config.for_txt(value), name).await
    }

    #[allow(dead_code)]
    pub async fn remove_txt_record(
        &self,
        config: &DnsPodConfig,
        name: &str,
        value: &str,
    ) -> Result<Option<String>, HostProvisionError> {
        self.remove_record(&config.for_txt(value), name).await
    }

    pub async fn ensure_record(
        &self,
        config: &DnsPodConfig,
        host: &str,
    ) -> Result<EnsuredRecord, HostProvisionError> {
        let existing = self
            .list_records(config, host)
            .await?
            .into_iter()
            .find(|record| {
                record.record_type == config.record_type
                    && record.line == config.record_line
                    && (record.name == subdomain(&config.domain, host).unwrap_or_default()
                        || record.name == host)
            });
        let Some(existing) = existing else {
            return Ok(EnsuredRecord {
                id: self.create_record(config, host).await?,
                updated: false,
            });
        };
        if existing.value == config.record_value && existing.ttl == config.ttl {
            return Ok(EnsuredRecord {
                id: existing.id,
                updated: false,
            });
        }
        self.update_record(config, host, &existing).await?;
        Ok(EnsuredRecord {
            id: existing.id,
            updated: true,
        })
    }

    pub async fn remove_record(
        &self,
        config: &DnsPodConfig,
        host: &str,
    ) -> Result<Option<String>, HostProvisionError> {
        let records = self.list_records(config, host).await?;
        let mut removed = None;
        for record in records.into_iter().filter(|record| {
            record.record_type == config.record_type
                && record.line == config.record_line
                && record.value == config.record_value
        }) {
            let record_id = record.id.parse::<u64>().map_err(|_| {
                HostProvisionError::Provider("dnspod returned an invalid record id".to_owned())
            })?;
            self.call(
                config,
                "DeleteRecord",
                json!({ "Domain": config.domain, "RecordId": record_id }),
            )
            .await?;
            removed = Some(record.id);
        }
        Ok(removed)
    }
}

fn subdomain(domain: &str, host: &str) -> Result<String, String> {
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();
    let host = host.trim().trim_end_matches('.');
    let host_lower = host.to_ascii_lowercase();
    if host_lower == domain {
        return Ok("@".to_owned());
    }
    let suffix = format!(".{domain}");
    if !host_lower.ends_with(&suffix) {
        return Err(format!("host {host} is not under DNSPod domain {domain}"));
    }
    Ok(host[..host.len() - suffix.len()].to_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};

    use super::*;

    type RequestLog = Arc<Mutex<Vec<(String, Value, HeaderMap)>>>;

    fn config(endpoint: &str) -> DnsPodConfig {
        DnsPodConfig::from_json(
            "example.com",
            &json!({
                "secret_id": "secret-id",
                "secret_key": "secret-key",
                "record_type": "A",
                "record_value": "203.0.113.7",
                "ttl": 300,
                "api_endpoint": endpoint,
            }),
        )
        .unwrap()
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
    fn tc3_signature_vector_is_stable() {
        assert_eq!(tc3_date(1_700_000_000), "2023-11-14");
        let signature = tc3_signature_for_test(
            "secret-key",
            "dnspod",
            "dnspod.tencentcloudapi.com",
            "POST",
            "/",
            "",
            "content-type:application/json; charset=utf-8\nhost:dnspod.tencentcloudapi.com\n",
            "content-type;host",
            "{}",
            1_700_000_000,
        );
        assert_eq!(signature.len(), 64);
        assert!(
            signature
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        );
    }

    #[test]
    fn config_rejects_missing_secret_and_invalid_types() {
        assert!(DnsPodConfig::from_json("example.com", &json!({})).is_err());
        assert!(
            DnsPodConfig::from_json(
                "example.com",
                &json!({
                    "secret_id": "id",
                    "secret_key": "key",
                    "record_type": "TXT",
                    "record_value": "v"
                }),
            )
            .is_err()
        );
    }

    #[test]
    fn subdomain_requires_an_exact_zone_suffix() {
        assert_eq!(subdomain("example.com", "www.example.com").unwrap(), "www");
        assert_eq!(subdomain("example.com", "example.com").unwrap(), "@");
        assert!(subdomain("example.com", "example.com.evil").is_err());
    }

    #[tokio::test]
    async fn ensure_reconciles_only_the_requested_type_and_signs_requests() {
        let actions: RequestLog = Arc::new(Mutex::new(Vec::new()));
        let state = actions.clone();
        let router = Router::new().route(
            "/",
            post(move |headers: HeaderMap, State(state): State<RequestLog>, Json(body): Json<Value>| async move {
                let action = headers
                    .get("x-tc-action")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .to_owned();
                state.lock().unwrap().push((action.clone(), body, headers.clone()));
                let response = match action.as_str() {
                    "DescribeRecordList" => json!({
                        "Response": { "RecordList": [
                            { "RecordId": 4, "Name": "www", "Type": "A", "Value": "198.51.100.8", "Line": "默认", "TTL": 60 },
                            { "RecordId": 5, "Name": "www", "Type": "TXT", "Value": "keep-me", "Line": "默认", "TTL": 60 }
                        ] }
                    }),
                    "ModifyRecord" => json!({ "Response": { "RequestId": "request-1" } }),
                    _ => json!({ "Response": {} }),
                };
                Json(response)
            }),
        ).with_state(state);
        let base = spawn(router).await;
        let ensured = DnsPod::with_base_url(base.clone())
            .ensure_record(&config(&base), "www.example.com")
            .await
            .unwrap();
        assert_eq!(ensured.id, "4");
        assert!(ensured.updated);

        let calls = actions.lock().unwrap();
        assert_eq!(
            calls.iter().map(|call| call.0.as_str()).collect::<Vec<_>>(),
            ["DescribeRecordList", "ModifyRecord"]
        );
        let auth = calls[0].2.get("authorization").unwrap().to_str().unwrap();
        assert!(auth.starts_with("TC3-HMAC-SHA256 Credential=secret-id/"));
        assert!(!auth.contains("secret-key"));
        assert_eq!(calls[1].1["RecordId"], 4);
        assert_eq!(calls[1].1["RecordType"], "A");
    }

    #[tokio::test]
    async fn remove_deletes_matching_records_but_leaves_unrelated_types() {
        let actions: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let state = actions.clone();
        let router = Router::new().route(
            "/",
            post(move |headers: HeaderMap, State(state): State<Arc<Mutex<Vec<String>>>>, Json(body): Json<Value>| async move {
                let action = headers
                    .get("x-tc-action")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default()
                    .to_owned();
                if action == "DeleteRecord" {
                    assert_eq!(body["RecordId"], 4);
                }
                state.lock().unwrap().push(action.clone());
                Json(if action == "DescribeRecordList" {
                    json!({ "Response": { "RecordList": [
                        { "RecordId": 4, "Name": "www", "Type": "A", "Value": "203.0.113.7", "Line": "默认", "TTL": 300 },
                        { "RecordId": 5, "Name": "www", "Type": "TXT", "Value": "keep-me", "Line": "默认", "TTL": 300 }
                    ] } })
                } else {
                    json!({ "Response": {} })
                })
            }),
        ).with_state(state);
        let base = spawn(router).await;
        let removed = DnsPod::with_base_url(base.clone())
            .remove_record(&config(&base), "www.example.com")
            .await
            .unwrap();
        assert_eq!(removed.as_deref(), Some("4"));
        assert_eq!(
            actions.lock().unwrap().as_slice(),
            ["DescribeRecordList", "DeleteRecord"]
        );
    }
}
