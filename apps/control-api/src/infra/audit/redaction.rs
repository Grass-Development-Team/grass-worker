const REDACTED: &str = "[REDACTED]";

pub fn redact_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(values) => serde_json::Value::Object(
            values
                .into_iter()
                .map(|(key, value)| {
                    let value = if sensitive_key(&key) {
                        serde_json::Value::String(REDACTED.to_owned())
                    } else {
                        redact_json(value)
                    };
                    (key, value)
                })
                .collect(),
        ),
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(redact_json).collect())
        }
        serde_json::Value::String(value) if sensitive_string(&value) => {
            serde_json::Value::String(REDACTED.to_owned())
        }
        other => other,
    }
}

fn sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    matches!(
        normalized.as_str(),
        "authorization"
            | "cookie"
            | "set_cookie"
            | "password"
            | "password_hash"
            | "passphrase"
            | "secret"
            | "secret_key"
            | "secret_ref"
            | "api_key"
            | "access_key"
            | "client_secret"
            | "signing_key"
            | "token"
            | "token_hash"
            | "access_token"
            | "refresh_token"
            | "csrf_token"
            | "session_id"
            | "private_key"
            | "master_key"
            | "credential"
            | "credentials"
            | "git_credentials"
            | "database_url"
            | "redis_url"
            | "connection_string"
    ) || normalized.ends_with("_password")
        || normalized.ends_with("_passphrase")
        || normalized.ends_with("_secret")
        || normalized.ends_with("_api_key")
        || normalized.ends_with("_access_key")
        || normalized.ends_with("_signing_key")
        || normalized.ends_with("_token")
        || normalized.ends_with("_cookie")
        || normalized.ends_with("_private_key")
        || normalized.ends_with("_credential")
        || normalized.ends_with("_credentials")
        || normalized.ends_with("_connection_string")
}

fn sensitive_string(value: &str) -> bool {
    if value
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer "))
    {
        return true;
    }

    let uppercase = value.to_ascii_uppercase();
    if uppercase.contains("-----BEGIN ") && uppercase.contains("PRIVATE KEY-----") {
        return true;
    }

    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    if url
        .query_pairs()
        .any(|(key, _)| sensitive_key(key.as_ref()))
    {
        return true;
    }
    match url.scheme() {
        "http" | "https" => !url.username().is_empty() || url.password().is_some(),
        "postgres" | "postgresql" | "redis" | "rediss" | "mysql" => true,
        _ => url.password().is_some(),
    }
}

/// Free text uses the same secret detection as metadata values.
pub(super) fn redact_text(value: String) -> String {
    let lower = value.to_ascii_lowercase();
    let embedded_secret = [
        "bearer ",
        "password=",
        "password:",
        "secret=",
        "token=",
        "cookie:",
        "-----begin ",
    ]
    .iter()
    .any(|marker| lower.contains(marker));
    if embedded_secret
        || value
            .split_whitespace()
            .any(|part| sensitive_string(part.trim_matches(['\"', '\'', '(', ')', ',', ';'])))
    {
        REDACTED.to_owned()
    } else {
        value
    }
}
