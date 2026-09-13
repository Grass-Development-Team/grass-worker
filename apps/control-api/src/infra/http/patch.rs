//! Nullable PATCH fields: missing means unchanged, null means clear.
use serde::{Deserialize, Deserializer};

/// Combine with `#[serde(default, deserialize_with = "...::nullable")]` on
/// an endpoint-local `Option<Option<T>>` field. Serde calls this only when
/// the field is present, so explicit null becomes `Some(None)`.
pub(crate) fn nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}
