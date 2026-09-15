//! DNS connection and ownership decisions for customer domains.
pub(crate) use crate::infra::dns::RecordMatch as DnsVerification;
use anyhow::Context;
use std::collections::BTreeSet;

use crate::infra::dns::{Resolver, canonical};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Ready,
    Unresolved,
    Mismatch,
    EntryUnavailable,
}

impl ConnectionState {
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

pub async fn connection(
    resolver: &Resolver,
    host: &str,
    target: &str,
) -> anyhow::Result<ConnectionState> {
    let expected = resolver.addresses(target).await?;
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
        let answers = resolver.query(&cursor, "CNAME").await?;
        let next = answers
            .iter()
            .find(|a| a.kind == 5 && canonical(&a.name) == cursor)
            .map(|a| canonical(&a.data));
        let Some(next) = next else {
            if had_cname {
                return Ok(ConnectionState::Mismatch);
            }
            // Apex ALIAS/ANAME and CNAME flattening expose address records only.
            let actual = resolver.addresses(host).await?;
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
            let actual = resolver.addresses(host).await?;
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
    resolver: &Resolver,
    host: &str,
    expected: &str,
) -> anyhow::Result<DnsVerification> {
    resolver
        .verify_record(
            &format!("_grass.{}", host.trim_end_matches('.')),
            "TXT",
            expected,
        )
        .await
        .context("Ownership DNS check could not be completed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::dns::{answer, fixture};
    use serde_json::json;

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
                connection(&resolver, "example.org", "entry.example.com")
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
                    json!({
                        "Status": 0,
                        "Answer": [
                            {
                                "name": "site.example.org.",
                                "type": 5,
                                "data": "alias.example.org.",
                            },
                            {
                                "name": "alias.example.org.",
                                "type": 5,
                                "data": "entry.example.com.",
                            },
                            {
                                "name": "entry.example.com.",
                                "type": 1,
                                "data": "203.0.113.1",
                            },
                        ],
                    }),
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
                connection(&resolver, "site.example.org", "entry.example.com")
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
            connection(&resolver, "missing.example.org", "entry.example.com")
                .await
                .unwrap(),
            ConnectionState::Unresolved
        );
        assert_eq!(
            connection(&resolver, "missing.example.org", "missing.entry.com")
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
            ("broken.example.org", "A", json!({ "Status": 2 })),
        ])
        .await;
        assert_eq!(
            connection(&resolver, "site.example.org", "entry.example.com")
                .await
                .unwrap(),
            ConnectionState::Mismatch
        );
        assert!(
            connection(&resolver, "broken.example.org", "entry.example.com")
                .await
                .is_err()
        );
        server.abort();
    }
}
