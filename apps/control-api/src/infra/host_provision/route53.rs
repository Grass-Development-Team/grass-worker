//! Amazon Route53 DNS client.
//!
//! Route53 uses the AWS Signature Version 4 protocol and an XML API. Record
//! sets are reconciled by exact name and type. When a set contains several
//! values, a transactional delete/create replaces only the observed record
//! set, retaining unrelated values and retrying concurrent modifications.

use std::sync::OnceLock;
use std::time::Duration;

use quick_xml::de::from_str;
use reqwest::Url;
use ring::hmac;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use super::{HostProvisionError, same_dns_name, same_record_value, send_dns_request};
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
        if hosted_zone_id.is_empty()
            || !hosted_zone_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err("config.hosted_zone_id must be an alphanumeric hosted zone id".to_owned());
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
        if !matches!(endpoint_url.scheme(), "http" | "https")
            || endpoint_url.host_str().is_none()
            || !endpoint_url.username().is_empty()
            || endpoint_url.password().is_some()
            || endpoint_url.path() != "/"
            || endpoint_url.query().is_some()
            || endpoint_url.fragment().is_some()
        {
            return Err("config.api_endpoint must be an http(s) origin without credentials, path, query, or fragment".to_owned());
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    #[serde(rename = "SetIdentifier")]
    set_identifier: Option<String>,
    #[serde(rename = "HealthCheckId")]
    health_check_id: Option<String>,
    #[serde(rename = "MultiValueAnswer", default)]
    multi_value_answer: bool,
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
    let signature = sigv4_digest(
        secret_access_key,
        region,
        service,
        date,
        timestamp,
        &canonical_request,
    );
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={access_key_id}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
    );
    (authorization, payload_hash)
}

fn sigv4_digest(
    secret_access_key: &str,
    region: &str,
    service: &str,
    date: &str,
    timestamp: &str,
    canonical_request: &str,
) -> String {
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
        sha256_hex(canonical_request)
    );
    let date_key = hmac_bytes(format!("AWS4{secret_access_key}").as_bytes(), date);
    let region_key = hmac_bytes(&date_key, region);
    let service_key = hmac_bytes(&region_key, service);
    let signing_key = hmac_bytes(&service_key, "aws4_request");
    hex::encode(hmac_bytes(&signing_key, &string_to_sign))
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
    let canonical_query = format!(
        "name={}&type={}",
        aws_uri_encode(query_name),
        aws_uri_encode(query_type)
    );
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
    format!(
        "{}.",
        host.trim().trim_end_matches('.').to_ascii_lowercase()
    )
}

/// SigV4 uses RFC3986 encoding, not HTML form encoding (`+` for space).
fn aws_uri_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn change_fragment(record: &RecordSet, action: &str) -> String {
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
        "<Change><Action>{action}</Action><ResourceRecordSet><Name>{}</Name><Type>{}</Type><TTL>{}</TTL><ResourceRecords>{values}</ResourceRecords></ResourceRecordSet></Change>",
        escape_xml(&record.name),
        escape_xml(&record.record_type),
        record.ttl,
    )
}

fn change_body(original: Option<&RecordSet>, desired: Option<&RecordSet>) -> String {
    let mut changes = String::new();
    if let Some(record) = original {
        changes.push_str(&change_fragment(record, "DELETE"));
    }
    if let Some(record) = desired {
        changes.push_str(&change_fragment(record, "CREATE"));
    }
    format!(
        "<ChangeResourceRecordSetsRequest xmlns=\"{XML_NAMESPACE}\"><ChangeBatch><Changes>{changes}</Changes></ChangeBatch></ChangeResourceRecordSetsRequest>"
    )
}

fn parse_record_set(
    xml: &str,
    name: &str,
    record_type: &str,
) -> Result<Option<RecordSet>, HostProvisionError> {
    let parsed: ListResponse = from_str(xml).map_err(|error| {
        HostProvisionError::Provider(format!("route53 returned invalid XML: {error}"))
    })?;
    let Some(record) = parsed
        .record_sets
        .and_then(|sets| sets.records.into_iter().next())
        .filter(|record| same_dns_name(&record.name, name) && record.record_type == record_type)
    else {
        return Ok(None);
    };
    // A simple-record template must not replace alias, weighted, failover,
    // health-checked or multivalue-routing records and silently lose policy.
    if record.set_identifier.is_some()
        || record.health_check_id.is_some()
        || record.multi_value_answer
    {
        return Err(HostProvisionError::Provider(
            "route53 record uses a routing policy that cannot be managed as a simple record"
                .to_owned(),
        ));
    }
    let ttl = record.ttl.ok_or_else(|| {
        HostProvisionError::Provider(
            "route53 alias records cannot be managed as simple records".to_owned(),
        )
    })?;
    let resource_records = record.resource_records.ok_or_else(|| {
        HostProvisionError::Provider("route53 returned a record set without values".to_owned())
    })?;
    Ok(Some(RecordSet {
        name: record.name,
        record_type: record.record_type.to_ascii_uppercase(),
        ttl,
        values: resource_records
            .records
            .into_iter()
            .map(|record| record.value)
            .collect(),
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
        for attempt in 0..3 {
            let response = send_dns_request(
                http_client()
                    .request(method.clone(), &url)
                    .header("content-type", "application/xml")
                    .header("host", &host)
                    .header("x-amz-content-sha256", &payload_hash)
                    .header("x-amz-date", &timestamp)
                    .header("authorization", &authorization)
                    .body(body.to_owned()),
            )
            .await
            .map_err(request_error)?;
            let status = response.status();
            let response_body = response.text().await.map_err(|error| {
                HostProvisionError::Provider(format!("route53 response read failed: {error}"))
            })?;
            if !status.is_success() {
                if attempt < 2
                    && (response_body.contains("<Code>Throttling</Code>")
                        || response_body.contains("<Code>PriorRequestNotComplete</Code>"))
                {
                    tokio::time::sleep(Duration::from_millis(100 << attempt)).await;
                    continue;
                }
                return Err(xml_api_error(&response_body, status.as_u16()));
            }
            return Ok(response_body);
        }
        unreachable!("the final attempt always returns")
    }

    async fn find_record_set(
        &self,
        config: &Route53Config,
        host: &str,
    ) -> Result<Option<RecordSet>, HostProvisionError> {
        let path = format!("/2013-04-01/hostedzone/{}/rrset", config.hosted_zone_id);
        let name = record_name(host);
        let name = aws_uri_encode(&name);
        let record_type = aws_uri_encode(&config.record_type);
        // Route53 returns records in DNS sort order starting at this exact
        // name/type. The first result proves presence or absence; subsequent
        // pages cannot contain a missed simple record of the same identity.
        let query = format!("maxitems=1&name={name}&type={record_type}");
        let response = self
            .request(config, reqwest::Method::GET, &path, &query, "")
            .await?;
        parse_record_set(&response, &record_name(host), &config.record_type)
    }

    async fn change_record_set(
        &self,
        config: &Route53Config,
        original: Option<&RecordSet>,
        desired: Option<&RecordSet>,
    ) -> Result<String, HostProvisionError> {
        let path = format!("/2013-04-01/hostedzone/{}/rrset", config.hosted_zone_id);
        let body = change_body(original, desired);
        let response = self
            .request(config, reqwest::Method::POST, &path, "", &body)
            .await?;
        Ok(parse_change_id(&response)
            .unwrap_or_else(|| format!("route53:{}", config.hosted_zone_id)))
    }

    pub async fn ensure_record(
        &self,
        config: &Route53Config,
        host: &str,
    ) -> Result<EnsuredRecord, HostProvisionError> {
        let desired_name = record_name(host);
        for attempt in 0..3 {
            let existing = self.find_record_set(config, host).await?;
            let mut record = existing.clone().unwrap_or_else(|| RecordSet {
                name: desired_name.clone(),
                record_type: config.record_type.clone(),
                ttl: config.ttl,
                values: Vec::new(),
            });
            let contains_value = record
                .values
                .iter()
                .any(|value| same_record_value(&config.record_type, value, &config.record_value));
            let matches = contains_value
                && record.ttl == config.ttl
                && (config.record_type != "CNAME" || record.values.len() == 1);
            if matches {
                return Ok(EnsuredRecord {
                    id: format!("route53:{}:{}", config.hosted_zone_id, record.name),
                    updated: false,
                });
            }
            if config.record_type == "CNAME" {
                record.values = vec![config.record_value.clone()];
            } else if !contains_value {
                record.values.push(config.record_value.clone());
            }
            record.ttl = config.ttl;
            match self
                .change_record_set(config, existing.as_ref(), Some(&record))
                .await
            {
                Ok(id) => {
                    return Ok(EnsuredRecord {
                        id,
                        updated: existing.is_some(),
                    });
                }
                Err(error) if attempt < 2 && is_change_conflict(&error) => {
                    tokio::time::sleep(Duration::from_millis(100 << attempt)).await;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("the final attempt always returns")
    }

    pub async fn remove_record(
        &self,
        config: &Route53Config,
        host: &str,
    ) -> Result<Option<String>, HostProvisionError> {
        for attempt in 0..3 {
            let Some(original) = self.find_record_set(config, host).await? else {
                return Ok(None);
            };
            let mut record = original.clone();
            let original_len = record.values.len();
            record.values.retain(|value| {
                !same_record_value(&config.record_type, value, &config.record_value)
            });
            if record.values.len() == original_len {
                return Ok(None);
            }
            let desired = (!record.values.is_empty()).then_some(&record);
            match self
                .change_record_set(config, Some(&original), desired)
                .await
            {
                Ok(id) => return Ok(Some(id)),
                Err(error) if attempt < 2 && is_change_conflict(&error) => {
                    tokio::time::sleep(Duration::from_millis(100 << attempt)).await;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("the final attempt always returns")
    }
}

fn is_change_conflict(error: &HostProvisionError) -> bool {
    matches!(error, HostProvisionError::Provider(message) if message.contains("(InvalidChangeBatch)"))
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
        assert_eq!(
            signature,
            "fee51c2f25ff3564810869958ecebd2a058a55b3c9d9533ba20c5f0001c639cb"
        );
        // AWS's official aws-c-auth signing test suite, v4/get-vanilla:
        // https://github.com/awslabs/aws-c-auth/tree/main/tests/aws-signing-test-suite/v4/get-vanilla
        let canonical_request = "GET\n/\n\nhost:example.amazonaws.com\nx-amz-date:20150830T123600Z\n\nhost;x-amz-date\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(
            sigv4_digest(
                "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
                "us-east-1",
                "service",
                "20150830",
                "20150830T123600Z",
                canonical_request
            ),
            "5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
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
            None,
            Some(&RecordSet {
                name: "a.example.com.".to_owned(),
                record_type: "A".to_owned(),
                ttl: 300,
                values: vec!["value<&".to_owned()],
            }),
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
        let replacement = body.split("<Action>CREATE</Action>").nth(1).unwrap();
        assert!(!replacement.contains("203.0.113.7"));
        assert!(body.contains("<Action>DELETE</Action>"));
    }

    #[derive(Default)]
    struct MockZone {
        record: Option<RecordSet>,
        replace_on_next_write: Option<RecordSet>,
        requests: Vec<String>,
    }

    #[derive(Deserialize)]
    struct MockChangeRequest {
        #[serde(rename = "ChangeBatch")]
        batch: MockChangeBatch,
    }

    #[derive(Deserialize)]
    struct MockChangeBatch {
        #[serde(rename = "Changes")]
        changes: MockChanges,
    }

    #[derive(Deserialize)]
    struct MockChanges {
        #[serde(rename = "Change")]
        changes: Vec<MockChange>,
    }

    #[derive(Deserialize)]
    struct MockChange {
        #[serde(rename = "Action")]
        action: String,
        #[serde(rename = "ResourceRecordSet")]
        record: ResourceRecordSetXml,
    }

    async fn mock_zone(zone: Arc<Mutex<MockZone>>) -> String {
        let router = Router::new().route("/2013-04-01/hostedzone/Z123/rrset", get(|State(zone): State<Arc<Mutex<MockZone>>>, Query(query): Query<HashMap<String, String>>| async move {
            assert_eq!(query.get("maxitems").map(String::as_str), Some("1"));
            let zone = zone.lock().unwrap();
            let record = zone.record.as_ref().map(|record| {
                let fragment = change_fragment(record, "CREATE");
                fragment.split_once("<ResourceRecordSet>").unwrap().1.split_once("</ResourceRecordSet>").unwrap().0.to_owned()
            }).map(|record| format!("<ResourceRecordSet>{record}</ResourceRecordSet>")).unwrap_or_default();
            format!("<ListResourceRecordSetsResponse><ResourceRecordSets>{record}</ResourceRecordSets></ListResourceRecordSetsResponse>")
        }).post(|State(zone): State<Arc<Mutex<MockZone>>>, body: String| async move {
            let mut zone = zone.lock().unwrap();
            zone.requests.push(body.clone());
            if let Some(concurrent) = zone.replace_on_next_write.take() {
                zone.record = Some(concurrent);
            }
            let request: MockChangeRequest = from_str(&body).unwrap();
            let mut candidate = zone.record.clone();
            for change in request.batch.changes.changes {
                let record = RecordSet {
                    name: change.record.name,
                    record_type: change.record.record_type,
                    ttl: change.record.ttl.unwrap(),
                    values: change.record.resource_records.unwrap().records.into_iter().map(|record| record.value).collect(),
                };
                let valid = match change.action.as_str() {
                    "DELETE" => candidate.as_ref() == Some(&record),
                    "CREATE" => candidate.is_none(),
                    action => panic!("unexpected non-transactional change {action}"),
                };
                if !valid {
                    return (axum::http::StatusCode::BAD_REQUEST, "<ErrorResponse><Error><Code>InvalidChangeBatch</Code><Message>record changed concurrently</Message></Error></ErrorResponse>".to_owned());
                }
                candidate = (change.action == "CREATE").then_some(record);
            }
            zone.record = candidate;
            (axum::http::StatusCode::OK, "<ChangeResourceRecordSetsResponse><ChangeInfo><Id>/change/mock</Id></ChangeInfo></ChangeResourceRecordSetsResponse>".to_owned())
        })).with_state(zone);
        spawn(router).await
    }

    #[tokio::test]
    async fn cname_replaces_the_old_target_and_remains_a_single_value() {
        let zone = Arc::new(Mutex::new(MockZone {
            record: Some(RecordSet {
                name: "app.example.com.".to_owned(),
                record_type: "CNAME".to_owned(),
                ttl: 300,
                values: vec!["old.example.com.".to_owned()],
            }),
            ..MockZone::default()
        }));
        let endpoint = mock_zone(zone.clone()).await;
        let mut config = config(&endpoint);
        config.record_type = "CNAME".to_owned();
        config.record_value = "new.example.com".to_owned();
        let dns = Route53::with_base_url(endpoint);
        assert!(
            dns.ensure_record(&config, "app.example.com")
                .await
                .unwrap()
                .updated
        );
        assert!(
            !dns.ensure_record(&config, "APP.example.com.")
                .await
                .unwrap()
                .updated
        );
        assert_eq!(
            zone.lock().unwrap().record.as_ref().unwrap().values,
            ["new.example.com"]
        );
        assert_eq!(zone.lock().unwrap().requests.len(), 1);
        config.record_value = "old.example.com".to_owned();
        assert_eq!(
            dns.remove_record(&config, "app.example.com").await.unwrap(),
            None
        );
    }

    #[test]
    fn closest_next_record_proves_absence_and_routing_policies_are_protected() {
        let xml = "<ListResourceRecordSetsResponse><ResourceRecordSets><ResourceRecordSet><Name>next.example.com.</Name><Type>A</Type></ResourceRecordSet></ResourceRecordSets><IsTruncated>true</IsTruncated><NextRecordName>z.example.com.</NextRecordName></ListResourceRecordSetsResponse>";
        assert!(
            parse_record_set(xml, "missing.example.com", "A")
                .unwrap()
                .is_none()
        );
        assert!(
            parse_record_set(xml, "next.example.com", "A")
                .unwrap_err()
                .to_string()
                .contains("alias")
        );
        let xml = "<ListResourceRecordSetsResponse><ResourceRecordSets><ResourceRecordSet><Name>app.example.com.</Name><Type>A</Type><SetIdentifier>weighted</SetIdentifier><TTL>60</TTL><ResourceRecords><ResourceRecord><Value>203.0.113.1</Value></ResourceRecord></ResourceRecords></ResourceRecordSet></ResourceRecordSets></ListResourceRecordSetsResponse>";
        assert!(
            parse_record_set(xml, "app.example.com", "A")
                .unwrap_err()
                .to_string()
                .contains("routing policy")
        );
    }
}
