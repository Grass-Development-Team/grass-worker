//! Shared repository endpoint parsing and network policy.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitTransport {
    Http,
    Https,
    Ssh,
    Git,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryEndpoint {
    pub transport: GitTransport,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub username: Option<String>,
    pub scp_like: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateTargetException {
    pub host: String,
    pub ip: IpAddr,
    pub port: u16,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RepositoryUrlError {
    #[error("repository URL is invalid")]
    Invalid,
    #[error("repository transport is not supported")]
    UnsupportedTransport,
    #[error("credentials must not be embedded in repository URLs")]
    EmbeddedCredential,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NetworkPolicyError {
    #[error("repository host did not resolve")]
    NoAddresses,
    #[error("repository target {ip}:{port} is not publicly routable")]
    NonPublicTarget { ip: IpAddr, port: u16 },
}

pub fn parse_repository_url(value: &str) -> Result<RepositoryEndpoint, RepositoryUrlError> {
    if value.is_empty()
        || value.trim() != value
        || value.chars().any(|character| character.is_control())
    {
        return Err(RepositoryUrlError::Invalid);
    }

    if !value.contains("://") {
        return parse_scp_like(value);
    }

    let parsed = url::Url::parse(value).map_err(|_| RepositoryUrlError::Invalid)?;
    let transport = match parsed.scheme() {
        "http" => GitTransport::Http,
        "https" => GitTransport::Https,
        "ssh" => GitTransport::Ssh,
        "git" => GitTransport::Git,
        _ => return Err(RepositoryUrlError::UnsupportedTransport),
    };
    if parsed.fragment().is_some() || parsed.query().is_some() {
        return Err(RepositoryUrlError::Invalid);
    }
    if parsed.password().is_some()
        || (!parsed.username().is_empty() && !matches!(transport, GitTransport::Ssh))
    {
        return Err(RepositoryUrlError::EmbeddedCredential);
    }

    let host = parsed
        .host_str()
        .ok_or(RepositoryUrlError::Invalid)
        .and_then(normalize_host)?;
    let port = parsed.port().unwrap_or(match transport {
        GitTransport::Http => 80,
        GitTransport::Https => 443,
        GitTransport::Ssh => 22,
        GitTransport::Git => 9418,
    });
    if port == 0 || parsed.path().is_empty() || parsed.path() == "/" {
        return Err(RepositoryUrlError::Invalid);
    }
    let username = (!parsed.username().is_empty())
        .then(|| normalize_ssh_username(parsed.username()))
        .transpose()?;

    Ok(RepositoryEndpoint {
        transport,
        host,
        port,
        path: parsed.path().to_owned(),
        username,
        scp_like: false,
    })
}

fn parse_scp_like(value: &str) -> Result<RepositoryEndpoint, RepositoryUrlError> {
    if value.starts_with('-')
        || (value.len() >= 3
            && value.as_bytes()[0].is_ascii_alphabetic()
            && value.as_bytes()[1] == b':'
            && matches!(value.as_bytes()[2], b'/' | b'\\'))
    {
        return Err(RepositoryUrlError::Invalid);
    }

    let delimiter = if value.starts_with('[') || value.contains("@[") {
        value
            .find("]:")
            .map(|closing_bracket| closing_bracket + 1)
            .ok_or(RepositoryUrlError::Invalid)?
    } else {
        value.find(':').ok_or(RepositoryUrlError::Invalid)?
    };
    let authority = &value[..delimiter];
    let path = &value[delimiter + 1..];
    if authority.is_empty()
        || path.is_empty()
        || path.starts_with(':')
        || path
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(RepositoryUrlError::Invalid);
    }

    let (username, raw_host) = match authority.rsplit_once('@') {
        Some((username, host)) => (Some(normalize_ssh_username(username)?), host),
        None => (None, authority),
    };
    let raw_host = raw_host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(raw_host);

    Ok(RepositoryEndpoint {
        transport: GitTransport::Ssh,
        host: normalize_host(raw_host)?,
        port: 22,
        path: path.to_owned(),
        username,
        scp_like: true,
    })
}

fn normalize_host(value: &str) -> Result<String, RepositoryUrlError> {
    let value = value.trim_end_matches('.');
    if value.is_empty()
        || value.starts_with('-')
        || value.chars().any(|character| character.is_whitespace())
    {
        return Err(RepositoryUrlError::Invalid);
    }
    if let Ok(address) = value.parse::<IpAddr>() {
        return Ok(address.to_string());
    }
    match url::Host::parse(value).map_err(|_| RepositoryUrlError::Invalid)? {
        url::Host::Domain(domain) => Ok(domain.to_ascii_lowercase()),
        url::Host::Ipv4(address) => Ok(address.to_string()),
        url::Host::Ipv6(address) => Ok(address.to_string()),
    }
}

fn normalize_ssh_username(value: &str) -> Result<String, RepositoryUrlError> {
    if value.is_empty()
        || value.starts_with('-')
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        return Err(RepositoryUrlError::Invalid);
    }
    Ok(value.to_owned())
}

pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => is_public_ipv6(ip),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !matches!(
        (a, b, c),
        (0, _, _)
            | (10, _, _)
            | (100, 64..=127, _)
            | (127, _, _)
            | (169, 254, _)
            | (172, 16..=31, _)
            | (192, 0, 0)
            | (192, 0, 2)
            | (192, 88, 99)
            | (192, 168, _)
            | (198, 18..=19, _)
            | (198, 51, 100)
            | (203, 0, 113)
            | (224..=255, _, _)
    )
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    if segments[..5] == [0, 0, 0, 0, 0] && segments[5] == 0xffff {
        let mapped = Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            segments[6] as u8,
            (segments[7] >> 8) as u8,
            segments[7] as u8,
        );
        return is_public_ipv4(mapped);
    }

    let globally_assigned = segments[0] & 0xe000 == 0x2000;
    let documentation = segments[0] == 0x2001 && segments[1] == 0x0db8;
    let benchmarking = segments[0] == 0x2001 && segments[1] == 0x0002;
    let orchid =
        segments[0] == 0x2001 && (segments[1] & 0xfff0 == 0x0010 || segments[1] & 0xfff0 == 0x0020);
    globally_assigned && !documentation && !benchmarking && !orchid
}

pub fn validate_resolved_targets(
    endpoint: &RepositoryEndpoint,
    addresses: &[IpAddr],
    exceptions: &[PrivateTargetException],
) -> Result<(), NetworkPolicyError> {
    if addresses.is_empty() {
        return Err(NetworkPolicyError::NoAddresses);
    }
    for &ip in addresses {
        let excepted = exceptions.iter().any(|exception| {
            exception.host.eq_ignore_ascii_case(&endpoint.host)
                && exception.ip == ip
                && exception.port == endpoint.port
        });
        if !is_public_ip(ip) && !excepted {
            return Err(NetworkPolicyError::NonPublicTarget {
                ip,
                port: endpoint.port,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn parses_supported_transports_and_effective_ports() {
        let cases = [
            ("http://example.com/repo.git", GitTransport::Http, 80),
            (
                "https://example.com:8443/repo.git",
                GitTransport::Https,
                8443,
            ),
            (
                "ssh://git@example.com:2222/repo.git",
                GitTransport::Ssh,
                2222,
            ),
            ("git@example.com:repo.git", GitTransport::Ssh, 22),
            ("git://example.com:19418/repo.git", GitTransport::Git, 19418),
        ];

        for (value, transport, port) in cases {
            let endpoint = parse_repository_url(value).unwrap();
            assert_eq!(endpoint.transport, transport, "{value}");
            assert_eq!(endpoint.port, port, "{value}");
            assert_eq!(endpoint.host, "example.com", "{value}");
        }
    }

    #[test]
    fn normalizes_hosts_and_preserves_ssh_usernames() {
        let uri = parse_repository_url("ssh://deploy@EXAMPLE.com.:22/org/repo.git").unwrap();
        assert_eq!(uri.host, "example.com");
        assert_eq!(uri.username.as_deref(), Some("deploy"));
        assert!(!uri.scp_like);

        let scp = parse_repository_url("git@[2001:db8::1]:org/repo.git").unwrap();
        assert_eq!(scp.host, "2001:db8::1");
        assert_eq!(scp.username.as_deref(), Some("git"));
        assert!(scp.scp_like);
    }

    #[test]
    fn rejects_local_unsupported_and_credential_bearing_urls() {
        for value in [
            "/srv/repo",
            "../repo",
            "C:\\repo",
            "file:///srv/repo",
            "ext::sh -c whoami",
            "ftp://example.com/repo.git",
            "https://token@example.com/repo.git",
            "https://user:secret@example.com/repo.git",
            "git://example.com/",
            "ssh://example.com/repo.git#main",
            "ssh://git@-oProxyCommand.example/repo.git",
            "git@-oProxyCommand.example:repo.git",
            "ssh://-oProxyCommand.example/repo.git",
            "git@evil@example.com:repo.git",
        ] {
            assert!(parse_repository_url(value).is_err(), "accepted {value}");
        }
    }

    #[test]
    fn classifies_public_and_non_public_addresses() {
        for ip in [
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            "1.1.1.1".parse().unwrap(),
            "2606:4700:4700::1111".parse().unwrap(),
        ] {
            assert!(is_public_ip(ip), "expected public: {ip}");
        }

        for ip in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            "10.0.0.1".parse().unwrap(),
            "100.64.0.1".parse().unwrap(),
            "169.254.1.1".parse().unwrap(),
            "192.0.2.1".parse().unwrap(),
            "198.18.0.1".parse().unwrap(),
            "203.0.113.1".parse().unwrap(),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            "fc00::1".parse().unwrap(),
            "fe80::1".parse().unwrap(),
            "2001:db8::1".parse().unwrap(),
            "ff02::1".parse().unwrap(),
            "::ffff:127.0.0.1".parse().unwrap(),
        ] {
            assert!(!is_public_ip(ip), "expected non-public: {ip}");
        }
    }

    #[test]
    fn exact_exception_requires_host_ip_and_port() {
        let endpoint = parse_repository_url("ssh://git.internal:2222/repo.git").unwrap();
        let private_ip: IpAddr = "10.0.0.8".parse().unwrap();
        let matching = PrivateTargetException {
            host: "git.internal".to_owned(),
            ip: private_ip,
            port: 2222,
        };
        assert!(
            validate_resolved_targets(&endpoint, &[private_ip], std::slice::from_ref(&matching))
                .is_ok()
        );

        for exception in [
            PrivateTargetException {
                host: "other.internal".to_owned(),
                ..matching.clone()
            },
            PrivateTargetException {
                ip: "10.0.0.9".parse().unwrap(),
                ..matching.clone()
            },
            PrivateTargetException {
                port: 22,
                ..matching.clone()
            },
        ] {
            assert_eq!(
                validate_resolved_targets(&endpoint, &[private_ip], &[exception]),
                Err(NetworkPolicyError::NonPublicTarget {
                    ip: private_ip,
                    port: 2222,
                })
            );
        }
    }

    #[test]
    fn every_resolved_address_must_be_allowed() {
        let endpoint = parse_repository_url("https://example.com/repo.git").unwrap();
        let public: IpAddr = "8.8.8.8".parse().unwrap();
        let private: IpAddr = "127.0.0.1".parse().unwrap();

        assert_eq!(
            validate_resolved_targets(&endpoint, &[], &[]),
            Err(NetworkPolicyError::NoAddresses)
        );
        assert_eq!(
            validate_resolved_targets(&endpoint, &[public, private], &[]),
            Err(NetworkPolicyError::NonPublicTarget {
                ip: private,
                port: 443,
            })
        );
    }
}
