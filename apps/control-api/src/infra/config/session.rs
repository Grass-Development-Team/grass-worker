use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionConfig {
    #[serde(default = "default_cookie_secure")]
    pub cookie_secure: bool,
    #[serde(default = "default_idle_ttl_seconds")]
    pub idle_ttl_seconds: u64,
    #[serde(default = "default_session_ttl_seconds")]
    pub session_ttl_seconds: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            cookie_secure: default_cookie_secure(),
            idle_ttl_seconds: default_idle_ttl_seconds(),
            session_ttl_seconds: default_session_ttl_seconds(),
        }
    }
}

const fn default_cookie_secure() -> bool {
    true
}

const fn default_idle_ttl_seconds() -> u64 {
    900
}

const fn default_session_ttl_seconds() -> u64 {
    2_592_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_session_defaults_match_auth_contract() {
        let config = SessionConfig::default();
        assert!(config.cookie_secure);
        assert_eq!(config.idle_ttl_seconds, 900);
        assert_eq!(config.session_ttl_seconds, 2_592_000);
    }
}
