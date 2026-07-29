use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuditConfig {
    /// Number of days request and domain audit events remain available.
    /// A value of zero keeps audit events permanently.
    #[serde(default = "default_retention_days")]
    pub retention_days: u64,
}

impl AuditConfig {
    pub fn retention_cutoff(&self, now: OffsetDateTime) -> Option<OffsetDateTime> {
        if self.retention_days == 0 {
            return None;
        }

        let max_days = (i64::MAX as u64) / 86_400;
        let days = self.retention_days.min(max_days) as i64;
        now.checked_sub(Duration::days(days))
    }
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            retention_days: default_retention_days(),
        }
    }
}

const fn default_retention_days() -> u64 {
    90
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_days_disables_expiration() {
        let config = AuditConfig { retention_days: 0 };

        assert_eq!(config.retention_cutoff(OffsetDateTime::UNIX_EPOCH), None);
    }
}
