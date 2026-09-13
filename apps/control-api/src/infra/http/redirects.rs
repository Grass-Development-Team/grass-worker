//! Validated return paths for local redirects.
pub(crate) fn safe_return_to(value: Option<&str>) -> String {
    value
        .filter(|value| {
            value.starts_with('/')
                && !value.starts_with("//")
                && !value.contains('\\')
                && value.len() <= 4096
                && !value.chars().any(char::is_control)
        })
        .unwrap_or("/")
        .to_owned()
}
