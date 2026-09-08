//! Amazon Route53 DNS client.
//!
//! Route53 uses the AWS Signature Version 4 protocol and an XML API. Record
//! sets are reconciled by exact name and type. When a set contains several
//! values, removing a Grass-managed value UPSERTs the remaining values so
//! unrelated records are retained.

use std::sync::OnceLock;
use std::time::Duration;

use quick_xml::de::from_str;
use reqwest::Url;
use ring::hmac;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use url::form_urlencoded;

use super::HostProvisionError;
use crate::infra::database::entity::host_source;

pub const PROVIDER_NAME: &str = "route53";
const DEFAULT_BASE_URL: &str = "https://route53.amazonaws.com";
const SERVICE_NAME: &str = "route53";
const SIGNING_REGION: &str = "us-east-1";
const XML_NAMESPACE: &str = "https://route53.amazonaws.com/doc/2013-04-01/";

#[derive(Debug, Clone)]
pub struct Route53Config {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub hosted_zone_id: String,
    pub record_type: String,
    pub record_value: String,
    pub ttl: u32,
    pub region: String,
    endpoint: String,
}

impl Route53Config {
    pub fn from_source(source: &host_source::Model) -> Result<Self, String> {
        Self::from_json(&source.config)
    }

    pub fn from_json(config: &Value) -> Result<Self, String> {
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
        let access_key_id = string_field("access_key_id")?;
        let secret_access_key = string_field("secret_access_key")?;
        let hosted_zone_id = normalize_zone_id(&string_field("hosted_zone_id")?);
        if hosted_zone_id.is_empty() {
            return Err("config.hosted_zone_id is required".to_owned());
        }
        let record_type = string_field("record_type")?.to_ascii_uppercase();
        if !matches!(record_type.as_str(), "A" | "AAAA" | "CNAME") {
            return Err("config.record_type must be A, AAAA, or CNAME".to_owned());
        }
        let record_value = string_field("record_value")?;
        let ttl = match object.get("ttl") {
            None | Some(Value::Null) => 300,
            Some(value) => value
                .as_u64()
                .filter(|ttl| (1..=172_800).contains(ttl))
                .and_then(|ttl| u32::try_from(ttl).ok())
                .ok_or_else(|| "config.ttl must be an integer from 1-172800 seconds".to_owned())?,
        };
        let region = object
            .get("region")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(SIGNING_REGION)
            .to_owned();
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
            access_key_id,
            secret_access_key,
            hosted_zone_id,
            record_type,
            record_value,
            ttl,
            region,
            endpoint,
        })
    }
}

fn normalize_zone_id(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('/')
        .strip_prefix("hostedzone/")
        .unwrap_or(value.trim().trim_start_matches('/'))
        .trim_start_matches('/')
        .to_owned()
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
struct RecordSet {
    name: String,
    record_type: String,
    ttl: u32,
    values: Vec<String>,
}

#[derive(Debug)]
pub struct EnsuredRecord {
    pub id: String,
    pub updated: bool,
}

#[derive(Debug, Deserialize)]
struct ListResponse {
    #[serde(rename = "ResourceRecordSets")]
    record_sets: Option<ResourceRecordSets>,
}

#[derive(Debug, Deserialize)]
struct ResourceRecordSets {
    #[serde(rename = "ResourceRecordSet", default)]
    records: Vec<ResourceRecordSetXml>,
}

#[derive(Debug, Deserialize)]
struct ResourceRecordSetXml {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Type")]
    record_type: String,
    #[serde(rename = "TTL")]
    ttl: Option<u32>,
    #[serde(rename = "ResourceRecords")]
    resource_records: Option<ResourceRecordsXml>,
}

#[derive(Debug, Deserialize)]
struct ResourceRecordsXml {
    #[serde(rename = "ResourceRecord", default)]
    records: Vec<ResourceRecordXml>,
}

#[derive(Debug, Deserialize)]
struct ResourceRecordXml {
    #[serde(rename = "Value")]
    value: String,
}

fn request_error(error: reqwest::Error) -> HostProvisionError {
    HostProvisionError::Provider(format!("route53 request failed: {}", error.without_url()))
}

fn endpoint_host(endpoint: &str) -> Result<String, HostProvisionError> {
    let url = Url::parse(endpoint)
        .map_err(|_| HostProvisionError::Provider("route53 endpoint URL is invalid".to_owned()))?;
    let host = url
        .host_str()
        .ok_or_else(|| HostProvisionError::Provider("route53 endpoint has no host".to_owned()))?;
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

fn amz_date() -> (String, String) {
    let now = OffsetDateTime::now_utc();
    let date = format!(
        "{:04}{:02}{:02}",
        now.year(),
        u8::from(now.month()),
        now.day()
    );
    let timestamp = format!(
        "{date}T{:02}{:02}{:02}Z",
        now.hour(),
        now.minute(),
        now.second()
    );
    (date, timestamp)
}

#[allow(clippy::too_many_arguments)]
fn sigv4_signature(
    access_key_id: &str,
    secret_access_key: &str,
    region: &str,
    service: &str,
    host: &str,
    method: &str,
    canonical_uri: &str,
    canonical_query: &str,
    body: &str,
    date: &str,
    timestamp: &str,
) -> (String, String) {
    let payload_hash = sha256_hex(body);
    let canonical_headers = format!(
        "content-type:application/xml\nhost:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{timestamp}\n"
    );
    let signed_headers = "content-type;host;x-amz-content-sha256;x-amz-date";
    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
        sha256_hex(canonical_request)
    );
    let date_key = hmac_bytes(format!("AWS4{secret_access_key}").as_bytes(), date);
    let region_key = hmac_bytes(&date_key, region);
    let service_key = hmac_bytes(&region_key, service);
    let signing_key = hmac_bytes(&service_key, "aws4_request");
    let signature = hex::encode(hmac_bytes(&signing_key, &string_to_sign));
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={access_key_id}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
    );
    (authorization, payload_hash)
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn signature_for_test(
    access_key_id: &str,
    secret_access_key: &str,
    service: &str,
    region: &str,
    timestamp: &str,
    method: &str,
    canonical_uri: &str,
    query_name: &str,
    query_type: &str,
) -> String {
    let mut query = form_urlencoded::Serializer::new(String::new());
    query.append_pair("name", query_name);
    query.append_pair("type", query_type);
    let canonical_query = query.finish();
    sigv4_signature(
        access_key_id,
        secret_access_key,
        region,
        service,
        "route53.amazonaws.com",
        method,
        canonical_uri,
        &canonical_query,
        "",
        &timestamp[..8],
        timestamp,
    )
    .0
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn record_name(host: &str) -> String {
    format!("{}.", host.trim().trim_end_matches('.'))
}

fn change_body(record: &RecordSet, action: &str) -> String {
    let values = record
        .values
        .iter()
        .map(|value| {
            format!(
                "<ResourceRecord><Value>{}</Value></ResourceRecord>",
                escape_xml(value)
            )
        })
        .collect::<String>();
    format!(
        "<ChangeResourceRecordSetsRequest xmlns=\"{XML_NAMESPACE}\"><ChangeBatch><Changes><Change><Action>{action}</Action><ResourceRecordSet><Name>{}</Name><Type>{}</Type><TTL>{}</TTL><ResourceRecords>{values}</ResourceRecords></ResourceRecordSet></Change></Changes></ChangeBatch></ChangeResourceRecordSetsRequest>",
        escape_xml(&record.name),
        escape_xml(&record.record_type),
        record.ttl,
    )
}

fn parse_record_set(xml: &str) -> Result<Option<RecordSet>, HostProvisionError> {
    let parsed: ListResponse = from_str(xml).map_err(|error| {
        HostProvisionError::Provider(format!("route53 returned invalid XML: {error}"))
    })?;
    Ok(parsed
        .record_sets
        .and_then(|sets| sets.records.into_iter().next())
        .map(|record| RecordSet {
            name: record.name,
            record_type: record.record_type.to_ascii_uppercase(),
            ttl: record.ttl.unwrap_or(300),
            values: record
                .resource_records
                .map(|records| {
                    records
                        .records
                        .into_iter()
                        .map(|record| record.value)
                        .collect()
                })
                .unwrap_or_default(),
        }))
}

/// Thin Route53 API client scoped to DNS record management.
#[derive(Debug, Clone)]
pub struct Route53 {
    base_url: String,
}

impl Default for Route53 {
    fn default() -> Self {
        Self::new()
    }
}

impl Route53 {
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

    fn endpoint(&self, config: &Route53Config) -> String {
        if config.endpoint == DEFAULT_BASE_URL {
            self.base_url.clone()
        } else {
            config.endpoint.clone()
        }
    }

    async fn request(
        &self,
        config: &Route53Config,
        method: reqwest::Method,
        path: &str,
        query: &str,
        body: &str,
    ) -> Result<String, HostProvisionError> {
        let endpoint = self.endpoint(config);
        let host = endpoint_host(&endpoint)?;
        let (date, timestamp) = amz_date();
        let (authorization, payload_hash) = sigv4_signature(
            &config.access_key_id,
            &config.secret_access_key,
            &config.region,
            SERVICE_NAME,
            &host,
            method.as_str(),
            path,
            query,
            body,
            &date,
            &timestamp,
        );
        let url = if query.is_empty() {
            format!("{endpoint}{path}")
        } else {
            format!("{endpoint}{path}?{query}")
        };
        let response = http_client()
            .request(method, url)
            .header("content-type", "application/xml")
            .header("host", &host)
            .header("x-amz-content-sha256", payload_hash)
            .header("x-amz-date", timestamp)
            .header("authorization", authorization)
            .body(body.to_owned())
            .send()
            .await
            .map_err(request_error)?;
        let status = response.status();
        let response_body = response.text().await.map_err(|error| {
            HostProvisionError::Provider(format!("route53 response read failed: {error}"))
        })?;
        if !status.is_success() {
            return Err(xml_api_error(&response_body, status.as_u16()));
        }
        Ok(response_body)
    }

    async fn find_record_set(
        &self,
        config: &Route53Config,
        host: &str,
    ) -> Result<Option<RecordSet>, HostProvisionError> {
        let path = format!("/2013-04-01/hostedzone/{}/rrset", config.hosted_zone_id);
        let name = record_name(host);
        let name = form_urlencoded::byte_serialize(name.as_bytes()).collect::<String>();
        let record_type =
            form_urlencoded::byte_serialize(config.record_type.as_bytes()).collect::<String>();
        let query = format!("name={name}&type={record_type}");
        let response = self
            .request(config, reqwest::Method::GET, &path, &query, "")
            .await?;
        let record = parse_record_set(&response)?;
        Ok(record.filter(|record| {
            record.name.eq_ignore_ascii_case(&record_name(host))
                && record.record_type == config.record_type
        }))
    }

    async fn change_record_set(
        &self,
        config: &Route53Config,
        record: &RecordSet,
        action: &str,
    ) -> Result<String, HostProvisionError> {
        let path = format!("/2013-04-01/hostedzone/{}/rrset", config.hosted_zone_id);
        let body = change_body(record, action);
        let response = self
            .request(config, reqwest::Method::POST, &path, "", &body)
            .await?;
        Ok(parse_change_id(&response)
            .unwrap_or_else(|| format!("route53:{}:{}", config.hosted_zone_id, record.name)))
    }

    pub async fn ensure_record(
        &self,
        config: &Route53Config,
        host: &str,
    ) -> Result<EnsuredRecord, HostProvisionError> {
        let desired_name = record_name(host);
        let existing = self.find_record_set(config, host).await?;
        let Some(mut record) = existing else {
            let record = RecordSet {
                name: desired_name,
                record_type: config.record_type.clone(),
                ttl: config.ttl,
                values: vec![config.record_value.clone()],
            };
            return Ok(EnsuredRecord {
                id: self.change_record_set(config, &record, "UPSERT").await?,
                updated: false,
            });
        };
        let matches = record.ttl == config.ttl
            && record.values.len() == 1
            && record.values.first().map(String::as_str) == Some(config.record_value.as_str());
        if matches {
            return Ok(EnsuredRecord {
                id: format!("route53:{}:{}", config.hosted_zone_id, record.name),
                updated: false,
            });
        }
        if !record
            .values
            .iter()
            .any(|value| value == &config.record_value)
        {
            record.values.push(config.record_value.clone());
        }
        record.ttl = config.ttl;
        Ok(EnsuredRecord {
            id: self.change_record_set(config, &record, "UPSERT").await?,
            updated: true,
        })
    }

    pub async fn remove_record(
        &self,
        config: &Route53Config,
        host: &str,
    ) -> Result<Option<String>, HostProvisionError> {
        let Some(mut record) = self.find_record_set(config, host).await? else {
            return Ok(None);
        };
        let original_len = record.values.len();
        record.values.retain(|value| value != &config.record_value);
        if record.values.len() == original_len {
            return Ok(None);
        }
        let action = if record.values.is_empty() {
            "DELETE"
        } else {
            "UPSERT"
        };
        let request_record = if action == "DELETE" {
            let mut request_record = record.clone();
            request_record.values = vec![config.record_value.clone()];
            request_record
        } else {
            record
        };
        let id = self
            .change_record_set(config, &request_record, action)
            .await?;
        Ok(Some(id))
    }
}

fn parse_change_id(xml: &str) -> Option<String> {
    let start = xml.find("<Id>")? + "<Id>".len();
    let end = xml[start..].find("</Id>")? + start;
    Some(xml[start..end].to_owned())
}

fn xml_api_error(xml: &str, status: u16) -> HostProvisionError {
    let code = xml
        .split_once("<Code>")
        .and_then(|(_, value)| value.split_once("</Code>"))
        .map(|(value, _)| value)
        .unwrap_or("UnknownError");
    let message = xml
        .split_once("<Message>")
        .and_then(|(_, value)| value.split_once("</Message>"))
        .map(|(value, _)| value)
        .unwrap_or("unknown error");
    HostProvisionError::Provider(format!(
        "route53 request failed ({status}): {message} ({code})"
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Router,
        extract::{Query, State},
        http::HeaderMap,
        routing::{get, post},
    };
    use std::collections::HashMap;

    use super::*;

    type RequestLog = Arc<Mutex<Vec<(String, String, String)>>>;

    fn config(endpoint: &str) -> Route53Config {
        Route53Config::from_json(&serde_json::json!({
            "access_key_id": "AKIDEXAMPLE",
            "secret_access_key": "secret-key",
            "hosted_zone_id": "/hostedzone/Z123",
            "record_type": "A",
            "record_value": "203.0.113.7",
            "ttl": 300,
            "api_endpoint": endpoint,
        }))
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
    fn sigv4_signature_vector_is_stable() {
        let authorization = signature_for_test(
            "AKIDEXAMPLE",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "route53",
            "us-east-1",
            "20150830T123600Z",
            "GET",
            "/2013-04-01/hostedzone/Z1PA6795UKMFR9/rrset",
            "a.example.com.",
            "A",
        );
        let signature = authorization
            .split("Signature=")
            .nth(1)
            .expect("signature field");
        assert_eq!(signature.len(), 64);
        assert!(
            signature
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        );
    }

    #[test]
    fn config_rejects_missing_secret_and_invalid_types() {
        assert!(Route53Config::from_json(&serde_json::json!({})).is_err());
        assert!(
            Route53Config::from_json(&serde_json::json!({
                "access_key_id": "id",
                "secret_access_key": "key",
                "hosted_zone_id": "zone",
                "record_type": "TXT",
                "record_value": "v"
            }))
            .is_err()
        );
    }

    #[test]
    fn zone_id_prefix_is_normalized() {
        assert_eq!(normalize_zone_id("/hostedzone/Z123"), "Z123");
        assert_eq!(normalize_zone_id("hostedzone/Z123"), "Z123");
    }

    #[test]
    fn change_body_escapes_values() {
        let body = change_body(
            &RecordSet {
                name: "a.example.com.".to_owned(),
                record_type: "A".to_owned(),
                ttl: 300,
                values: vec!["value<&".to_owned()],
            },
            "UPSERT",
        );
        assert!(body.contains("value&lt;&amp;"));
    }

    #[tokio::test]
    async fn ensure_updates_target_set_and_preserves_other_values() {
        let requests: RequestLog = Arc::new(Mutex::new(Vec::new()));
        let state = requests.clone();
        let router = Router::new()
            .route(
                "/2013-04-01/hostedzone/Z123/rrset",
                get(move |State(state): State<RequestLog>, headers: HeaderMap, Query(query): Query<HashMap<String, String>>| async move {
                    let query = format!("name={}&type={}", query.get("name").cloned().unwrap_or_default(), query.get("type").cloned().unwrap_or_default());
                    let authorization = headers
                        .get("authorization")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default()
                        .to_owned();
                    state.lock().unwrap().push(("GET".to_owned(), query, authorization));
                    let xml = format!(
                        "<ListResourceRecordSetsResponse xmlns=\"{XML_NAMESPACE}\"><ResourceRecordSets><ResourceRecordSet><Name>a.example.com.</Name><Type>A</Type><TTL>60</TTL><ResourceRecords><ResourceRecord><Value>198.51.100.2</Value></ResourceRecord><ResourceRecord><Value>198.51.100.3</Value></ResourceRecord></ResourceRecords></ResourceRecordSet></ResourceRecordSets></ListResourceRecordSetsResponse>"
                    );
                    ([("content-type", "application/xml")], xml)
                }),
            )
            .route(
                "/2013-04-01/hostedzone/Z123/rrset",
                post(move |State(state): State<RequestLog>, body: String| async move {
                    state.lock().unwrap().push(("POST".to_owned(), body, String::new()));
                    ([("content-type", "application/xml")], format!("<ChangeResourceRecordSetsResponse xmlns=\"{XML_NAMESPACE}\"><ChangeInfo><Id>/change/C1</Id></ChangeInfo></ChangeResourceRecordSetsResponse>"))
                }),
            )
            .with_state(state);
        let base = spawn(router).await;
        let ensured = Route53::with_base_url(base.clone())
            .ensure_record(&config(&base), "a.example.com")
            .await
            .unwrap();
        assert_eq!(ensured.id, "/change/C1");
        assert!(ensured.updated);

        let requests = requests.lock().unwrap();
        let authorization = &requests[0].2;
        assert!(authorization.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"));
        assert!(!authorization.contains("secret-key"));
        assert!(requests[1].1.contains("198.51.100.2"));
        assert!(requests[1].1.contains("198.51.100.3"));
        assert!(requests[1].1.contains("203.0.113.7"));
    }

    #[tokio::test]
    async fn remove_deletes_only_the_configured_value() {
        let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let state = requests.clone();
        let router = Router::new()
            .route(
                "/2013-04-01/hostedzone/Z123/rrset",
                get(|| async {
                    format!(
                        "<ListResourceRecordSetsResponse xmlns=\"{XML_NAMESPACE}\"><ResourceRecordSets><ResourceRecordSet><Name>a.example.com.</Name><Type>A</Type><TTL>300</TTL><ResourceRecords><ResourceRecord><Value>203.0.113.7</Value></ResourceRecord><ResourceRecord><Value>198.51.100.4</Value></ResourceRecord></ResourceRecords></ResourceRecordSet></ResourceRecordSets></ListResourceRecordSetsResponse>"
                    )
                }),
            )
            .route(
                "/2013-04-01/hostedzone/Z123/rrset",
                post(move |State(state): State<Arc<Mutex<Vec<String>>>>, body: String| async move {
                    state.lock().unwrap().push(body);
                    ([("content-type", "application/xml")], format!("<ChangeResourceRecordSetsResponse xmlns=\"{XML_NAMESPACE}\"><ChangeInfo><Id>/change/C2</Id></ChangeInfo></ChangeResourceRecordSetsResponse>"))
                }),
            )
            .with_state(state);
        let base = spawn(router).await;
        let removed = Route53::with_base_url(base.clone())
            .remove_record(&config(&base), "a.example.com")
            .await
            .unwrap();
        assert_eq!(removed.as_deref(), Some("/change/C2"));
        let body = &requests.lock().unwrap()[0];
        assert!(body.contains("198.51.100.4"));
        assert!(!body.contains("203.0.113.7"));
        assert!(body.contains("<Action>UPSERT</Action>"));
    }
}
