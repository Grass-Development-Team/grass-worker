//! Host provisioning abstraction.
//!
//! Callers describe the host they want; a provisioner decides how the DNS
//! side is fulfilled. Wildcard sources only need the internal binding, the
//! manual mode leaves configuration to an operator, and the DNS provider
//! mode is a configurable placeholder for real provider integrations.

pub mod service;

use crate::infra::database::entity::{HostBindingStatus, HostSourceKind, host_source};

pub struct ProvisionProjectHostInput<'a> {
    pub host: &'a str,
    pub source: &'a host_source::Model,
}

pub struct DeprovisionProjectHostInput<'a> {
    #[allow(dead_code)] // Read by DNS provider implementations that delete records.
    pub host: &'a str,
    pub source: &'a host_source::Model,
}

/// Outcome of a provisioning attempt. All three states are recorded in
/// `host_provision_events`; only `status` decides whether the binding serves
/// traffic.
#[derive(Debug)]
pub struct ProvisionedHost {
    pub status: HostBindingStatus,
    pub provider_request_id: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum HostProvisionError {
    #[allow(dead_code)] // Reserved for future host source kinds.
    #[error("host source kind is not supported: {0}")]
    UnsupportedSource(String),
    #[error("dns provider request failed: {0}")]
    Provider(String),
}

pub trait HostProvisioner: Send + Sync {
    fn provision_project_host(
        &self,
        input: ProvisionProjectHostInput<'_>,
    ) -> impl Future<Output = Result<ProvisionedHost, HostProvisionError>> + Send;

    fn deprovision_project_host(
        &self,
        input: DeprovisionProjectHostInput<'_>,
    ) -> impl Future<Output = Result<(), HostProvisionError>> + Send;
}

/// Wildcard DNS is configured out of band (`*.base_domain` already points at
/// the Node serve entry), so provisioning only records the internal binding.
pub struct WildcardHostProvisioner;

impl HostProvisioner for WildcardHostProvisioner {
    async fn provision_project_host(
        &self,
        _input: ProvisionProjectHostInput<'_>,
    ) -> Result<ProvisionedHost, HostProvisionError> {
        Ok(ProvisionedHost {
            status: HostBindingStatus::Active,
            provider_request_id: None,
            message: Some("wildcard DNS resolves this host without provider calls".to_owned()),
        })
    }

    async fn deprovision_project_host(
        &self,
        _input: DeprovisionProjectHostInput<'_>,
    ) -> Result<(), HostProvisionError> {
        Ok(())
    }
}

/// Manual sources wait for an operator to configure DNS; bindings stay
/// pending until an operator marks them resolved by re-running provision.
pub struct ManualHostProvisioner;

impl HostProvisioner for ManualHostProvisioner {
    async fn provision_project_host(
        &self,
        input: ProvisionProjectHostInput<'_>,
    ) -> Result<ProvisionedHost, HostProvisionError> {
        Ok(ProvisionedHost {
            status: HostBindingStatus::Pending,
            provider_request_id: None,
            message: Some(format!(
                "configure DNS for {} manually, then re-run provisioning",
                input.host
            )),
        })
    }

    async fn deprovision_project_host(
        &self,
        _input: DeprovisionProjectHostInput<'_>,
    ) -> Result<(), HostProvisionError> {
        Ok(())
    }
}

/// Configurable placeholder for DNS provider integrations. The source
/// config's `placeholder_result` key decides the simulated outcome
/// (`active`, `pending`, or `failed`) until a real provider client lands.
pub struct DnsProviderHostProvisioner;

impl HostProvisioner for DnsProviderHostProvisioner {
    async fn provision_project_host(
        &self,
        input: ProvisionProjectHostInput<'_>,
    ) -> Result<ProvisionedHost, HostProvisionError> {
        let provider = input.source.provider.as_deref().unwrap_or("none");
        let placeholder = input
            .source
            .config
            .get("placeholder_result")
            .and_then(|value| value.as_str())
            .unwrap_or("pending");

        match placeholder {
            "active" => Ok(ProvisionedHost {
                status: HostBindingStatus::Active,
                provider_request_id: Some(format!("placeholder-{provider}")),
                message: Some(format!(
                    "placeholder {provider} provisioning treated the record as created"
                )),
            }),
            "failed" => Err(HostProvisionError::Provider(format!(
                "placeholder {provider} provisioning is configured to fail"
            ))),
            _ => Ok(ProvisionedHost {
                status: HostBindingStatus::Pending,
                provider_request_id: None,
                message: Some(format!(
                    "{provider} DNS provider integration is not configured yet; \
                     binding stays pending until provisioning is retried"
                )),
            }),
        }
    }

    async fn deprovision_project_host(
        &self,
        _input: DeprovisionProjectHostInput<'_>,
    ) -> Result<(), HostProvisionError> {
        Ok(())
    }
}

/// Dispatches to the concrete provisioner based on the host source kind.
pub struct CompositeHostProvisioner {
    wildcard: WildcardHostProvisioner,
    manual: ManualHostProvisioner,
    dns_provider: DnsProviderHostProvisioner,
}

impl Default for CompositeHostProvisioner {
    fn default() -> Self {
        Self::new()
    }
}

impl CompositeHostProvisioner {
    pub fn new() -> Self {
        Self {
            wildcard: WildcardHostProvisioner,
            manual: ManualHostProvisioner,
            dns_provider: DnsProviderHostProvisioner,
        }
    }
}

impl HostProvisioner for CompositeHostProvisioner {
    async fn provision_project_host(
        &self,
        input: ProvisionProjectHostInput<'_>,
    ) -> Result<ProvisionedHost, HostProvisionError> {
        match input.source.kind {
            HostSourceKind::Wildcard => self.wildcard.provision_project_host(input).await,
            HostSourceKind::Manual => self.manual.provision_project_host(input).await,
            HostSourceKind::DnsProvider => self.dns_provider.provision_project_host(input).await,
        }
    }

    async fn deprovision_project_host(
        &self,
        input: DeprovisionProjectHostInput<'_>,
    ) -> Result<(), HostProvisionError> {
        match input.source.kind {
            HostSourceKind::Wildcard => self.wildcard.deprovision_project_host(input).await,
            HostSourceKind::Manual => self.manual.deprovision_project_host(input).await,
            HostSourceKind::DnsProvider => self.dns_provider.deprovision_project_host(input).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::*;

    fn source(kind: HostSourceKind, config: serde_json::Value) -> host_source::Model {
        host_source::Model {
            id: Uuid::nil(),
            kind,
            label: "test".to_owned(),
            base_domain: "grass.test".to_owned(),
            enabled: true,
            allows_auto_assign: true,
            is_default: true,
            provider: Some("cloudflare".to_owned()),
            config,
            deleted_at: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[tokio::test]
    async fn wildcard_sources_activate_immediately() {
        let source = source(HostSourceKind::Wildcard, serde_json::json!({}));
        let provisioned = CompositeHostProvisioner::new()
            .provision_project_host(ProvisionProjectHostInput {
                host: "demo.grass.test",
                source: &source,
            })
            .await
            .unwrap();
        assert_eq!(provisioned.status, HostBindingStatus::Active);
    }

    #[tokio::test]
    async fn manual_sources_stay_pending() {
        let source = source(HostSourceKind::Manual, serde_json::json!({}));
        let provisioned = CompositeHostProvisioner::new()
            .provision_project_host(ProvisionProjectHostInput {
                host: "demo.grass.test",
                source: &source,
            })
            .await
            .unwrap();
        assert_eq!(provisioned.status, HostBindingStatus::Pending);
    }

    #[tokio::test]
    async fn dns_provider_placeholder_honours_configured_outcome() {
        let pending = source(HostSourceKind::DnsProvider, serde_json::json!({}));
        let provisioned = CompositeHostProvisioner::new()
            .provision_project_host(ProvisionProjectHostInput {
                host: "demo.grass.test",
                source: &pending,
            })
            .await
            .unwrap();
        assert_eq!(provisioned.status, HostBindingStatus::Pending);

        let failing = source(
            HostSourceKind::DnsProvider,
            serde_json::json!({ "placeholder_result": "failed" }),
        );
        let error = CompositeHostProvisioner::new()
            .provision_project_host(ProvisionProjectHostInput {
                host: "demo.grass.test",
                source: &failing,
            })
            .await
            .unwrap_err();
        assert!(matches!(error, HostProvisionError::Provider(_)));
    }
}
