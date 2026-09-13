//! DNS-over-HTTPS transport, bounded address queries and raw record matching.
use anyhow::ensure;
use serde::Deserialize;
use std::{collections::BTreeSet, net::IpAddr, time::Duration};
use subtle::ConstantTimeEq;

pub struct Resolver {
    client: reqwest::Client,
    endpoint: String,
}

#[derive(Deserialize)]
pub(crate) struct Answer {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) kind: u16,
    pub(crate) data: String,
}

#[derive(Deserialize)]
struct Response {
    #[serde(rename = "Status")]
    status: u16,
    #[serde(rename = "Answer", default)]
    answers: Vec<Answer>,
}

pub(crate) fn canonical(name: &str) -> String {
    name.trim_end_matches('.').to_ascii_lowercase()
}

impl Resolver {
    pub fn new() -> anyhow::Result<Self> {
        Self::with_endpoint("https://cloudflare-dns.com/dns-query")
    }

    pub(crate) fn with_endpoint(endpoint: &str) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()?,
            endpoint: endpoint.to_owned(),
        })
    }

    pub(crate) async fn query(&self, name: &str, kind: &str) -> anyhow::Result<Vec<Answer>> {
        let response = self
            .client
            .get(&self.endpoint)
            .query(&[("name", name), ("type", kind)])
            .header("accept", "application/dns-json")
            .send()
            .await?
            .error_for_status()?;
        let bytes = response.bytes().await?;
        ensure!(
            bytes.len() <= 65_536,
            "DNS response exceeds the supported size"
        );
        let response: Response = serde_json::from_slice(&bytes)?;
        ensure!(
            response.status == 0 || response.status == 3,
            "Public DNS resolver is temporarily unavailable"
        );
        Ok(response.answers)
    }

    pub async fn addresses(&self, name: &str) -> anyhow::Result<BTreeSet<IpAddr>> {
        let (v4, v6) = tokio::try_join!(self.query(name, "A"), self.query(name, "AAAA"))?;
        let answers = v4.into_iter().chain(v6).collect::<Vec<_>>();
        let mut names = BTreeSet::from([canonical(name)]);
        for _ in 0..16 {
            let before = names.len();
            for answer in &answers {
                if answer.kind == 5 && names.contains(&canonical(&answer.name)) {
                    names.insert(canonical(&answer.data));
                }
            }
            if before == names.len() {
                break;
            }
        }
        Ok(answers
            .iter()
            .filter(|a| matches!(a.kind, 1 | 28) && names.contains(&canonical(&a.name)))
            .filter_map(|a| a.data.parse().ok())
            .collect())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordMatch {
    Verified,
    Missing,
    Mismatch,
}

pub async fn verify_dns_record_at(
    client: &reqwest::Client,
    endpoint: &str,
    name: &str,
    record_type: &str,
    expected: &str,
) -> anyhow::Result<RecordMatch> {
    let response = client
        .get(endpoint)
        .query(&[("name", name), ("type", record_type)])
        .header("accept", "application/dns-json")
        .send()
        .await?;
    if !response.status().is_success() {
        anyhow::bail!("DNS TXT query returned {}", response.status());
    }
    let body: serde_json::Value = response.json().await?;
    if body
        .get("Status")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|status| status != 0 && status != 3)
    {
        anyhow::bail!("DNS resolver returned an unsuccessful status");
    }
    let answers = body
        .get("Answer")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut found = false;
    for answer in answers {
        let wanted = if record_type == "CNAME" { 5 } else { 16 };
        if answer.get("type").and_then(serde_json::Value::as_u64) != Some(wanted) {
            continue;
        }
        let Some(data) = answer.get("data").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let text = if record_type == "CNAME" {
            data.trim().trim_end_matches('.').to_ascii_lowercase()
        } else {
            data.trim().trim_matches('"').replace("\" \"", "")
        };
        let value = text.as_str();
        if value.as_bytes().ct_eq(expected.as_bytes()).into() {
            return Ok(RecordMatch::Verified);
        }
        found = true;
    }
    Ok(if found {
        RecordMatch::Mismatch
    } else {
        RecordMatch::Missing
    })
}

impl Resolver {
    pub(crate) async fn verify_record(
        &self,
        name: &str,
        kind: &str,
        expected: &str,
    ) -> anyhow::Result<RecordMatch> {
        verify_dns_record_at(&self.client, &self.endpoint, name, kind, expected).await
    }
}
