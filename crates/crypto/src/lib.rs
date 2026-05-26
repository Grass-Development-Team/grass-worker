//! grass-crypto internal crate.

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};

/// Errors that can occur during password hashing or verification.
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("password hashing or verification failed")]
    PasswordHash(#[from] argon2::password_hash::Error),
}

/// Hashes a password using Argon2id with default parameters and a random salt.
pub fn hash_password(password: &str) -> Result<String, CryptoError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(password.as_bytes(), &salt)?;
    Ok(hash.to_string())
}

/// Verifies a password against a stored Argon2id hash.
pub fn verify_password(password: &str, hash: &str) -> Result<bool, CryptoError> {
    let parsed_hash = PasswordHash::new(hash)?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_password() {
        let password = "correct-horse-battery-staple";
        let hash = hash_password(password).unwrap();
        assert!(verify_password(password, &hash).unwrap());
        assert!(!verify_password("wrong-password", &hash).unwrap());
    }
}
