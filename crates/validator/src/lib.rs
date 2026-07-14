//! Shared validation and normalization helpers.

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SlugError {
    Empty,
    InvalidCharacter,
    TooLong,
}

impl std::fmt::Display for SlugError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => formatter.write_str("slug is required"),
            Self::InvalidCharacter => {
                formatter.write_str("slug may contain only ASCII letters, numbers, and separators")
            }
            Self::TooLong => formatter.write_str("slug must not exceed 120 characters"),
        }
    }
}

impl std::error::Error for SlugError {}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum EmailError {
    Invalid,
    TooLong,
}

impl std::fmt::Display for EmailError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid => formatter.write_str("invalid email address"),
            Self::TooLong => formatter.write_str("email address must not exceed 320 characters"),
        }
    }
}

impl std::error::Error for EmailError {}

pub fn normalize_email(value: &str) -> Result<String, EmailError> {
    let value = value.trim();
    if value.len() > 320 {
        return Err(EmailError::TooLong);
    }
    let Some((local, domain)) = value.split_once('@') else {
        return Err(EmailError::Invalid);
    };
    if local.is_empty()
        || domain.is_empty()
        || domain.contains('@')
        || value.chars().any(|character| {
            !character.is_ascii() || character.is_ascii_whitespace() || character.is_ascii_control()
        })
    {
        return Err(EmailError::Invalid);
    }
    Ok(value.to_ascii_lowercase())
}

pub fn normalize_slug(value: &str) -> Result<String, SlugError> {
    let mut normalized = String::with_capacity(value.len());
    let mut separator_pending = false;

    for character in value.trim().chars() {
        if character.is_ascii_alphanumeric() {
            if separator_pending && !normalized.is_empty() {
                normalized.push('-');
            }
            separator_pending = false;
            normalized.push(character.to_ascii_lowercase());
        } else if character == '-' || character == '_' || character.is_ascii_whitespace() {
            separator_pending = !normalized.is_empty();
        } else {
            return Err(SlugError::InvalidCharacter);
        }

        if normalized.len() > 120 {
            return Err(SlugError::TooLong);
        }
    }

    if normalized.is_empty() {
        return Err(SlugError::Empty);
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_slug_case_and_separators() {
        assert_eq!(
            normalize_slug("  My__Team -- Name  ").unwrap(),
            "my-team-name"
        );
    }

    #[test]
    fn rejects_empty_invalid_and_oversized_slugs() {
        assert_eq!(normalize_slug("---"), Err(SlugError::Empty));
        assert_eq!(
            normalize_slug("team.name"),
            Err(SlugError::InvalidCharacter)
        );
        assert_eq!(normalize_slug(&"a".repeat(121)), Err(SlugError::TooLong));
    }

    #[test]
    fn normalizes_and_validates_email_addresses() {
        assert_eq!(
            normalize_email(" User@Example.COM ").unwrap(),
            "user@example.com"
        );
        assert_eq!(normalize_email("missing-at"), Err(EmailError::Invalid));
        assert_eq!(normalize_email("@example.com"), Err(EmailError::Invalid));
        assert_eq!(
            normalize_email("user@@example.com"),
            Err(EmailError::Invalid)
        );
        assert_eq!(
            normalize_email(&format!("{}@example.com", "a".repeat(309))),
            Err(EmailError::TooLong)
        );
    }
}
