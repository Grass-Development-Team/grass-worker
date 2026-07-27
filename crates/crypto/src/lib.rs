//! grass-crypto internal crate.

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    aead,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};

const AEAD_VERSION: u8 = 1;
const AEAD_ALGORITHM: &str = "AES-256-GCM";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AeadEnvelope {
    pub version: u8,
    pub algorithm: String,
    pub key_id: String,
    pub nonce: String,
    pub ciphertext: String,
}

/// Errors that can occur during password hashing or verification.
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("password hashing or verification failed")]
    PasswordHash(#[from] argon2::password_hash::Error),
    #[error("invalid encryption key")]
    InvalidEncryptionKey,
    #[error("secret encryption failed")]
    EncryptionFailed,
    #[error("secret envelope is invalid or authentication failed")]
    InvalidEnvelope,
}

pub fn encrypt_secret(
    key_id: &str,
    key: &[u8],
    plaintext: &[u8],
    associated_data: &[u8],
) -> Result<AeadEnvelope, CryptoError> {
    if key_id.is_empty() {
        return Err(CryptoError::InvalidEncryptionKey);
    }
    let key = encryption_key(key)?;
    let mut nonce_bytes = [0_u8; 12];
    SystemRandom::new()
        .fill(&mut nonce_bytes)
        .map_err(|_| CryptoError::EncryptionFailed)?;
    let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);
    let mut ciphertext = plaintext.to_vec();
    key.seal_in_place_append_tag(nonce, aead::Aad::from(associated_data), &mut ciphertext)
        .map_err(|_| CryptoError::EncryptionFailed)?;

    Ok(AeadEnvelope {
        version: AEAD_VERSION,
        algorithm: AEAD_ALGORITHM.to_owned(),
        key_id: key_id.to_owned(),
        nonce: URL_SAFE_NO_PAD.encode(nonce_bytes),
        ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
    })
}

pub fn decrypt_secret(
    envelope: &AeadEnvelope,
    key: &[u8],
    associated_data: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if envelope.version != AEAD_VERSION
        || envelope.algorithm != AEAD_ALGORITHM
        || envelope.key_id.is_empty()
    {
        return Err(CryptoError::InvalidEnvelope);
    }
    let key = encryption_key(key)?;
    let nonce_bytes: [u8; 12] = URL_SAFE_NO_PAD
        .decode(&envelope.nonce)
        .map_err(|_| CryptoError::InvalidEnvelope)?
        .try_into()
        .map_err(|_| CryptoError::InvalidEnvelope)?;
    let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);
    let mut ciphertext = URL_SAFE_NO_PAD
        .decode(&envelope.ciphertext)
        .map_err(|_| CryptoError::InvalidEnvelope)?;
    let plaintext = key
        .open_in_place(nonce, aead::Aad::from(associated_data), &mut ciphertext)
        .map_err(|_| CryptoError::InvalidEnvelope)?;
    Ok(plaintext.to_vec())
}

fn encryption_key(key: &[u8]) -> Result<aead::LessSafeKey, CryptoError> {
    let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, key)
        .map_err(|_| CryptoError::InvalidEncryptionKey)?;
    Ok(aead::LessSafeKey::new(unbound))
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

    #[test]
    fn aead_round_trip_uses_random_nonces() {
        let key = [7_u8; 32];
        let first = encrypt_secret("primary", &key, b"private-key", b"credential:1").unwrap();
        let second = encrypt_secret("primary", &key, b"private-key", b"credential:1").unwrap();

        assert_eq!(first.version, 1);
        assert_eq!(first.algorithm, "AES-256-GCM");
        assert_eq!(first.key_id, "primary");
        assert_ne!(first.nonce, second.nonce);
        assert_ne!(first.ciphertext, second.ciphertext);
        assert_eq!(
            decrypt_secret(&first, &key, b"credential:1").unwrap(),
            b"private-key"
        );
    }

    #[test]
    fn aead_rejects_wrong_keys_tampering_and_wrong_context() {
        let key = [9_u8; 32];
        let envelope = encrypt_secret("primary", &key, b"token", b"team:1").unwrap();

        assert!(decrypt_secret(&envelope, &[8_u8; 32], b"team:1").is_err());
        assert!(decrypt_secret(&envelope, &key, b"team:2").is_err());

        let mut tampered = envelope;
        tampered.ciphertext.push('A');
        assert!(decrypt_secret(&tampered, &key, b"team:1").is_err());
    }

    #[test]
    fn aead_requires_a_256_bit_key_and_known_envelope_version() {
        assert!(encrypt_secret("primary", &[1_u8; 31], b"token", b"context").is_err());

        let key = [1_u8; 32];
        let mut envelope = encrypt_secret("primary", &key, b"token", b"context").unwrap();
        envelope.version = 2;
        assert!(decrypt_secret(&envelope, &key, b"context").is_err());
    }
}
