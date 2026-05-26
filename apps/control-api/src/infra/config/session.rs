use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionConfig {
    #[serde(default)]
    pub cookie_secure: bool,
    #[serde(default = "default_session_ttl_seconds")]
    pub session_ttl_seconds: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            cookie_secure: false,
            session_ttl_seconds: default_session_ttl_seconds(),
        }
    }
}

const fn default_session_ttl_seconds() -> u64 {
    2_592_000
}
