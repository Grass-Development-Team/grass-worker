//! RFC 3339 timestamp serialization for API responses.
//!
//! `time`'s default serde representation is not parseable by JavaScript's
//! `Date`, so every timestamp leaving the HTTP layer goes through here.

use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub trait ToRfc3339 {
    fn to_rfc3339_value(&self) -> Value;
}

impl ToRfc3339 for OffsetDateTime {
    fn to_rfc3339_value(&self) -> Value {
        json!(
            self.format(&Rfc3339)
                .unwrap_or_else(|_| self.unix_timestamp().to_string())
        )
    }
}

impl ToRfc3339 for Option<OffsetDateTime> {
    fn to_rfc3339_value(&self) -> Value {
        match self {
            Some(at) => at.to_rfc3339_value(),
            None => Value::Null,
        }
    }
}

/// Formats any timestamp (optional or not) as an RFC 3339 JSON value.
pub fn ts(value: impl ToRfc3339) -> Value {
    value.to_rfc3339_value()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_serialize_as_rfc3339() {
        let at = OffsetDateTime::from_unix_timestamp(1_753_500_000).unwrap();
        let value = ts(at);
        let text = value.as_str().unwrap();
        assert!(text.ends_with('Z'), "expected UTC marker in {text}");
        assert!(text.starts_with("2025-") || text.starts_with("2026-"));
        assert_eq!(ts(None::<OffsetDateTime>), Value::Null);
        assert_eq!(ts(Some(at)), value);
    }
}
