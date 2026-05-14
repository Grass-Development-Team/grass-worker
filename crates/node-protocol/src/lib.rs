//! grass-node-protocol internal crate.

/// Returns the crate identifier for diagnostics and smoke tests.
#[must_use]
pub const fn crate_name() -> &'static str {
    "node_protocol"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_crate_name() {
        assert_eq!(crate_name(), "node_protocol");
    }
}
