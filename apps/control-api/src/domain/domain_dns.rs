//! Public DNS inspection for manually configured entries and customer domains.
use anyhow::{Context, ensure};
use serde::Deserialize;
use std::{collections::BTreeSet, net::IpAddr, time::Duration};

pub struct Resolver {
    client: reqwest::Client,
    endpoint: String,
}
#[derive(Deserialize)]
struct Answer {
    name: String,
    #[serde(rename = "type")]
    kind: u16,
    data: String,
}
#[derive(Deserialize)]
struct Response {
    #[serde(rename = "Status")]
    status: u16,
    #[serde(rename = "Answer", default)]
    answers: Vec<Answer>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Ready,
    Unresolved,
    Mismatch,
    EntryUnavailable,
}
impl ConnectionState {
    pub fn status(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Unresolved => "unresolved",
            Self::Mismatch => "mismatch",
            Self::EntryUnavailable => "entry_unavailable",
        }
    }
    pub fn message(self) -> Option<&'static str> {
        match self {
            Self::Ready => None,
            Self::Unresolved => Some(
                "No public DNS address was found. Add the displayed CNAME record and wait for DNS propagation.",
            ),
            Self::Mismatch => Some(
                "This domain does not point to the selected regional entry. Correct its CNAME or flattened DNS records.",
            ),
            Self::EntryUnavailable => Some(
                "The platform's regional entry has no public DNS address. An administrator must configure its DNS records.",
            ),
        }
    }
}
fn canonical(name: &str) -> String {
    name.trim_end_matches('.').to_ascii_lowercase()
}
impl Resolver {
    pub fn new() -> anyhow::Result<Self> {
        Self::with_endpoint("https://cloudflare-dns.com/dns-query")
    }
    fn with_endpoint(endpoint: &str) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()?,
            endpoint: endpoint.to_owned(),
        })
    }
    async fn query(&self, name: &str, kind: &str) -> anyhow::Result<Vec<Answer>> {
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
    pub async fn connection(&self, host: &str, target: &str) -> anyhow::Result<ConnectionState> {
        let expected = self.addresses(target).await?;
        if expected.is_empty() {
            return Ok(ConnectionState::EntryUnavailable);
        }
        let mut cursor = canonical(host);
        let target = canonical(target);
        let mut seen = BTreeSet::new();
        let mut had_cname = false;
        for _ in 0..16 {
            if !seen.insert(cursor.clone()) {
                anyhow::bail!("DNS CNAME chain contains a loop");
            }
            let answers = self.query(&cursor, "CNAME").await?;
            let next = answers
                .iter()
                .find(|a| a.kind == 5 && canonical(&a.name) == cursor)
                .map(|a| canonical(&a.data));
            let Some(next) = next else {
                if had_cname {
                    return Ok(ConnectionState::Mismatch);
                }
                // Apex ALIAS/ANAME and CNAME flattening expose address records only.
                let actual = self.addresses(host).await?;
                return Ok(if actual.is_empty() {
                    ConnectionState::Unresolved
                } else if actual.is_subset(&expected) {
                    ConnectionState::Ready
                } else {
                    ConnectionState::Mismatch
                });
            };
            had_cname = true;
            if next == target {
                let actual = self.addresses(host).await?;
                return Ok(if actual.is_empty() {
                    ConnectionState::Unresolved
                } else if actual.is_subset(&expected) {
                    ConnectionState::Ready
                } else {
                    ConnectionState::Mismatch
                });
            }
            cursor = next;
        }
        anyhow::bail!("DNS CNAME chain exceeds 16 links")
    }
    pub async fn ownership(
        &self,
        host: &str,
        expected: &str,
    ) -> anyhow::Result<super::ingress::DnsVerification> {
        super::ingress::verify_dns_txt_at(&self.client, &self.endpoint, host, expected)
            .await
            .context("Ownership DNS check could not be completed")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::collections::HashMap;
    pub async fn fixture(
        records: Vec<(&str, &str, Value)>,
    ) -> (Resolver, tokio::task::JoinHandle<()>) {
        let records = records
            .into_iter()
            .map(|(name, kind, value)| ((name.to_owned(), kind.to_owned()), value))
            .collect::<HashMap<_, _>>();
        let app = axum::Router::new().route("/dns", axum::routing::get(|axum::extract::State(records): axum::extract::State<HashMap<(String, String), Value>>, axum::extract::Query(q): axum::extract::Query<HashMap<String,String>>| async move { axum::Json(records.get(&(q["name"].clone(), q["type"].clone())).cloned().unwrap_or_else(|| json!({"Status":0,"Answer":[]}))) })).with_state(records);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let resolver =
            Resolver::with_endpoint(&format!("http://{}/dns", listener.local_addr().unwrap()))
                .unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (resolver, server)
    }
    pub fn answer(name: &str, kind: u16, data: &str) -> Value {
        json!({"Status":0,"Answer":[{"name":name,"type":kind,"data":data}]})
    }
    #[tokio::test]
    async fn requires_the_selected_entry_and_accepts_apex_flattening() {
        for (actual, expected) in [
            ("203.0.113.1", ConnectionState::Ready),
            ("203.0.113.2", ConnectionState::Mismatch),
        ] {
            let (resolver, server) = fixture(vec![
                (
                    "entry.example.com",
                    "A",
                    answer("entry.example.com", 1, "203.0.113.1"),
                ),
                ("example.org", "A", answer("example.org", 1, actual)),
            ])
            .await;
            assert_eq!(
                resolver
                    .connection("example.org", "entry.example.com")
                    .await
                    .unwrap(),
                expected
            );
            server.abort();
        }
    }
    #[tokio::test]
    async fn accepts_cname_chains_and_rejects_a_different_entry_on_shared_ip() {
        for (target, expected) in [
            ("ENTRY.EXAMPLE.COM.", ConnectionState::Ready),
            ("wrong.example.com.", ConnectionState::Mismatch),
        ] {
            let (resolver, server) = fixture(vec![
                (
                    "entry.example.com",
                    "A",
                    answer("entry.example.com", 1, "203.0.113.1"),
                ),
                (
                    "site.example.org",
                    "A",
                    json!({"Status":0,"Answer":[
                        {"name":"site.example.org.","type":5,"data":"alias.example.org."},
                        {"name":"alias.example.org.","type":5,"data":"entry.example.com."},
                        {"name":"entry.example.com.","type":1,"data":"203.0.113.1"}
                    ]}),
                ),
                (
                    "site.example.org",
                    "CNAME",
                    answer("site.example.org", 5, "alias.example.org."),
                ),
                (
                    "alias.example.org",
                    "CNAME",
                    answer("alias.example.org", 5, target),
                ),
            ])
            .await;
            assert_eq!(
                resolver
                    .connection("site.example.org", "entry.example.com")
                    .await
                    .unwrap(),
                expected
            );
            server.abort();
        }
    }
    #[tokio::test]
    async fn distinguishes_unresolved_customer_dns_from_missing_entry_dns() {
        let (resolver, server) = fixture(vec![(
            "entry.example.com",
            "A",
            answer("entry.example.com", 1, "203.0.113.1"),
        )])
        .await;
        assert_eq!(
            resolver
                .connection("missing.example.org", "entry.example.com")
                .await
                .unwrap(),
            ConnectionState::Unresolved
        );
        assert_eq!(
            resolver
                .connection("missing.example.org", "missing.entry.com")
                .await
                .unwrap(),
            ConnectionState::EntryUnavailable
        );
        server.abort();
    }
    #[tokio::test]
    async fn wrong_ipv6_and_resolver_failures_do_not_pass_verification() {
        let (resolver, server) = fixture(vec![
            (
                "entry.example.com",
                "A",
                answer("entry.example.com", 1, "203.0.113.1"),
            ),
            (
                "site.example.org",
                "A",
                answer("site.example.org", 1, "203.0.113.1"),
            ),
            (
                "site.example.org",
                "AAAA",
                answer("site.example.org", 28, "2001:db8::2"),
            ),
            ("broken.example.org", "A", json!({"Status":2})),
        ])
        .await;
        assert_eq!(
            resolver
                .connection("site.example.org", "entry.example.com")
                .await
                .unwrap(),
            ConnectionState::Mismatch
        );
        assert!(
            resolver
                .connection("broken.example.org", "entry.example.com")
                .await
                .is_err()
        );
        server.abort();
    }
}
