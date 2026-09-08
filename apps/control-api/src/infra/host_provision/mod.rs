//! Host provisioning abstraction.
//!
//! Callers describe the host they want; a provisioner decides how the DNS
//! side is fulfilled. Wildcard sources only need the internal binding, the
//! manual mode leaves configuration to an operator, and the DNS provider
//! mode drives a real provider API.

pub mod cloudflare;
pub mod dnspod;
pub mod route53;
pub mod service;

use crate::infra::database::entity::{HostBindingStatus, HostSourceKind, host_source};

pub struct ProvisionProjectHostInput<'a> {
    pub host: &'a str,
    pub source: &'a host_source::Model,
}

pub struct DeprovisionProjectHostInput<'a> {
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

/// Creates one DNS record per provisioned host through the provider named
/// on the source. Provider credentials remain write-only in the admin API.
pub struct DnsProviderHostProvisioner {
    cloudflare: cloudflare::CloudflareDns,
    dnspod: dnspod::DnsPod,
    route53: route53::Route53,
}

impl DnsProviderHostProvisioner {
    pub fn new() -> Self {
        Self {
            cloudflare: cloudflare::CloudflareDns::new(),
            dnspod: dnspod::DnsPod::new(),
            route53: route53::Route53::new(),
        }
    }

    #[allow(dead_code)]
    pub async fn ensure_txt_record(
        &self,
        provider: &str,
        config: &serde_json::Value,
        zone: &str,
        name: &str,
        value: &str,
    ) -> Result<String, HostProvisionError> {
        match provider.trim().to_ascii_lowercase().as_str() {
            cloudflare::PROVIDER_NAME => {
                let parsed = cloudflare::CloudflareConfig::from_json(&txt_config(config, value))
                    .map_err(HostProvisionError::Provider)?;
                Ok(self
                    .cloudflare
                    .ensure_txt_record(&parsed, name, value)
                    .await?
                    .id)
            }
            dnspod::PROVIDER_NAME => {
                let parsed = dnspod::DnsPodConfig::from_json(zone, &txt_config(config, value))
                    .map_err(HostProvisionError::Provider)?;
                Ok(self
                    .dnspod
                    .ensure_txt_record(&parsed, name, value)
                    .await?
                    .id)
            }
            route53::PROVIDER_NAME => {
                let parsed = route53::Route53Config::from_json(&txt_config(config, value))
                    .map_err(HostProvisionError::Provider)?;
                Ok(self
                    .route53
                    .ensure_txt_record(&parsed, name, value)
                    .await?
                    .id)
            }
            other => Err(Self::unsupported(Some(other))),
        }
    }

    #[allow(dead_code)]
    pub async fn remove_txt_record(
        &self,
        provider: &str,
        config: &serde_json::Value,
        zone: &str,
        name: &str,
        value: &str,
    ) -> Result<Option<String>, HostProvisionError> {
        match provider.trim().to_ascii_lowercase().as_str() {
            cloudflare::PROVIDER_NAME => {
                let parsed = cloudflare::CloudflareConfig::from_json(&txt_config(config, value))
                    .map_err(HostProvisionError::Provider)?;
                self.cloudflare
                    .remove_txt_record(&parsed, name, value)
                    .await
            }
            dnspod::PROVIDER_NAME => {
                let parsed = dnspod::DnsPodConfig::from_json(zone, &txt_config(config, value))
                    .map_err(HostProvisionError::Provider)?;
                self.dnspod.remove_txt_record(&parsed, name, value).await
            }
            route53::PROVIDER_NAME => {
                let parsed = route53::Route53Config::from_json(&txt_config(config, value))
                    .map_err(HostProvisionError::Provider)?;
                self.route53.remove_txt_record(&parsed, name, value).await
            }
            other => Err(Self::unsupported(Some(other))),
        }
    }

    fn unsupported(provider: Option<&str>) -> HostProvisionError {
        HostProvisionError::UnsupportedSource(format!(
            "dns provider '{}' is not supported (supported: {})",
            provider.unwrap_or("none"),
            supported_provider_names(),
        ))
    }
}

/// Add the record fields required by the provider parsers without requiring
/// ACME configuration to contain a project-host record template.
fn txt_config(config: &serde_json::Value, value: &str) -> serde_json::Value {
    let mut object = config.as_object().cloned().unwrap_or_default();
    object.insert("record_type".to_owned(), serde_json::json!("TXT"));
    object.insert("record_value".to_owned(), serde_json::json!(value));
    serde_json::Value::Object(object)
}

pub fn supported_provider_names() -> &'static str {
    "cloudflare, dnspod, route53"
}

impl Default for DnsProviderHostProvisioner {
    fn default() -> Self {
        Self::new()
    }
}

impl HostProvisioner for DnsProviderHostProvisioner {
    async fn provision_project_host(
        &self,
        input: ProvisionProjectHostInput<'_>,
    ) -> Result<ProvisionedHost, HostProvisionError> {
        let provider = input
            .source
            .provider
            .as_deref()
            .map(str::to_ascii_lowercase);
        match provider.as_deref() {
            Some(cloudflare::PROVIDER_NAME) => {
                let config = cloudflare::CloudflareConfig::from_source(input.source)
                    .map_err(HostProvisionError::Provider)?;
                let ensured = self.cloudflare.ensure_record(&config, input.host).await?;
                Ok(ProvisionedHost {
                    status: HostBindingStatus::Active,
                    provider_request_id: Some(ensured.id),
                    message: Some(format!(
                        "cloudflare {} record for {} {}",
                        config.record_type,
                        input.host,
                        if ensured.updated {
                            "updated"
                        } else {
                            "created"
                        },
                    )),
                })
            }
            Some(dnspod::PROVIDER_NAME) => {
                let config = dnspod::DnsPodConfig::from_source(input.source)
                    .map_err(HostProvisionError::Provider)?;
                let ensured = self.dnspod.ensure_record(&config, input.host).await?;
                Ok(ProvisionedHost {
                    status: HostBindingStatus::Active,
                    provider_request_id: Some(ensured.id),
                    message: Some(format!(
                        "dnspod {} record for {} {}",
                        config.record_type,
                        input.host,
                        if ensured.updated {
                            "updated"
                        } else {
                            "created"
                        },
                    )),
                })
            }
            Some(route53::PROVIDER_NAME) => {
                let config = route53::Route53Config::from_source(input.source)
                    .map_err(HostProvisionError::Provider)?;
                let ensured = self.route53.ensure_record(&config, input.host).await?;
                Ok(ProvisionedHost {
                    status: HostBindingStatus::Active,
                    provider_request_id: Some(ensured.id),
                    message: Some(format!(
                        "route53 {} record for {} {}",
                        config.record_type,
                        input.host,
                        if ensured.updated {
                            "updated"
                        } else {
                            "created"
                        },
                    )),
                })
            }
            other => Err(Self::unsupported(other)),
        }
    }

    async fn deprovision_project_host(
        &self,
        input: DeprovisionProjectHostInput<'_>,
    ) -> Result<(), HostProvisionError> {
        let provider = input
            .source
            .provider
            .as_deref()
            .map(str::to_ascii_lowercase);
        match provider.as_deref() {
            Some(cloudflare::PROVIDER_NAME) => {
                let config = cloudflare::CloudflareConfig::from_source(input.source)
                    .map_err(HostProvisionError::Provider)?;
                self.cloudflare.remove_record(&config, input.host).await?;
                Ok(())
            }
            Some(dnspod::PROVIDER_NAME) => {
                let config = dnspod::DnsPodConfig::from_source(input.source)
                    .map_err(HostProvisionError::Provider)?;
                self.dnspod.remove_record(&config, input.host).await?;
                Ok(())
            }
            Some(route53::PROVIDER_NAME) => {
                let config = route53::Route53Config::from_source(input.source)
                    .map_err(HostProvisionError::Provider)?;
                self.route53.remove_record(&config, input.host).await?;
                Ok(())
            }
            other => Err(Self::unsupported(other)),
        }
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
            dns_provider: DnsProviderHostProvisioner::new(),
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
            region: "default".to_owned(),
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
    async fn dns_provider_requires_a_supported_provider_name() {
        let mut unsupported = source(HostSourceKind::DnsProvider, serde_json::json!({}));
        unsupported.provider = Some("unsupported".to_owned());
        let error = CompositeHostProvisioner::new()
            .provision_project_host(ProvisionProjectHostInput {
                host: "demo.grass.test",
                source: &unsupported,
            })
            .await
            .unwrap_err();
        assert!(matches!(error, HostProvisionError::UnsupportedSource(_)));

        let mut unset = source(HostSourceKind::DnsProvider, serde_json::json!({}));
        unset.provider = None;
        let error = CompositeHostProvisioner::new()
            .provision_project_host(ProvisionProjectHostInput {
                host: "demo.grass.test",
                source: &unset,
            })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("cloudflare"), "{error}");
    }

    #[tokio::test]
    async fn cloudflare_sources_fail_clearly_on_incomplete_config() {
        let incomplete = source(
            HostSourceKind::DnsProvider,
            serde_json::json!({ "zone_id": "zone1" }),
        );
        let error = CompositeHostProvisioner::new()
            .provision_project_host(ProvisionProjectHostInput {
                host: "demo.grass.test",
                source: &incomplete,
            })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("api_token"), "{error}");
    }
}
