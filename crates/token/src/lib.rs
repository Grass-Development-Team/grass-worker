//! grass-token internal crate.

use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// Generates a cryptographically secure random token encoded as base64url without padding.
///
/// The token is 32 random bytes (256 bits), encoded as base64url which produces
/// exactly 43 characters.
pub fn generate_token() -> String {
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    engine.encode(bytes)
}

/// Hashes a token using SHA-256 and returns the hex-encoded digest.
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    hex::encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_token_length() {
        let token = generate_token();
        assert_eq!(token.len(), 43, "base64url of 256 bits should be 43 chars");
    }

    #[test]
    fn generated_tokens_are_unique() {
        let a = generate_token();
        let b = generate_token();
        assert_ne!(a, b);
    }

    #[test]
    fn hash_token_is_consistent() {
        let token = "test-token-value";
        let h1 = hash_token(token);
        let h2 = hash_token(token);
        assert_eq!(h1, h2);
    }
}
