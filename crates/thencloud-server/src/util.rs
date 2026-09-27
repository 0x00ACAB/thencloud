use std::time::{SystemTime, UNIX_EPOCH};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use thencloud_crypto::api::B64;
use thencloud_crypto::{KEY_LEN, NONCE_LEN, TAG_LEN};

use crate::error::{AppError, Result};

/// A symmetric key wrapped with XChaCha20-Poly1305.
pub const WRAPPED_KEY_LEN: usize = NONCE_LEN + KEY_LEN + TAG_LEN;
/// A key sealed to an X25519 public key (ephemeral pubkey + wrapped key).
pub const SEALED_KEY_LEN: usize = 32 + WRAPPED_KEY_LEN;
pub const MAX_METADATA_LEN: usize = 16 * 1024;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before 1970")
        .as_secs() as i64
}

pub fn random_token(bytes: usize) -> String {
    thencloud_crypto::b64_encode(&thencloud_crypto::random_bytes(bytes))
}

pub fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn sha256(b: &[u8]) -> Vec<u8> {
    Sha256::digest(b).to_vec()
}

pub fn hmac(secret: &[u8], msg: &[u8]) -> Vec<u8> {
    let mut m = Hmac::<Sha256>::new_from_slice(secret).expect("any key length");
    m.update(msg);
    m.finalize().into_bytes().to_vec()
}

pub fn hmac_verify(secret: &[u8], msg: &[u8], tag: &[u8]) -> bool {
    let mut m = Hmac::<Sha256>::new_from_slice(secret).expect("any key length");
    m.update(msg);
    m.verify_slice(tag).is_ok()
}

/// Client-supplied ids must be canonical lowercase hyphenated UUIDs. They
/// end up in file paths and AEAD associated data.
pub fn check_id(id: &str, what: &str) -> Result<()> {
    match uuid::Uuid::parse_str(id) {
        Ok(u) if u.hyphenated().to_string() == id => Ok(()),
        _ => Err(AppError::bad(format!("{what} must be a lowercase UUID"))),
    }
}

pub const NAME_TAG_LEN: usize = 32;

pub fn check_name_tag(tag: &Option<B64>) -> Result<()> {
    match tag {
        Some(t) => check_len(t, NAME_TAG_LEN, "name_tag"),
        None => Ok(()),
    }
}

pub fn check_len(b: &[u8], len: usize, what: &str) -> Result<()> {
    if b.len() != len {
        return Err(AppError::bad(format!("{what} must be {len} bytes")));
    }
    Ok(())
}

pub fn check_metadata(b: &[u8]) -> Result<()> {
    if b.len() < NONCE_LEN + TAG_LEN || b.len() > MAX_METADATA_LEN {
        return Err(AppError::bad("enc_metadata has an invalid size"));
    }
    Ok(())
}

/// Argon2id PHC hash, computed off the async runtime.
pub async fn hash_secret(secret: Vec<u8>) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::encode_b64(&thencloud_crypto::random_bytes(16))
            .map_err(|e| AppError::Internal(e.to_string()))?;
        argon2::Argon2::default()
            .hash_password(&secret, &salt)
            .map(|h| h.to_string())
            .map_err(|e| AppError::Internal(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

pub async fn verify_secret(secret: Vec<u8>, phc: String) -> Result<bool> {
    tokio::task::spawn_blocking(move || {
        let parsed = PasswordHash::new(&phc).map_err(|e| AppError::Internal(e.to_string()))?;
        Ok(argon2::Argon2::default()
            .verify_password(&secret, &parsed)
            .is_ok())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

pub fn normalize_username(raw: &str) -> Result<String> {
    let u = raw.trim().to_lowercase();
    let ok = (3..=32).contains(&u.len())
        && u.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && u.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !ok {
        return Err(AppError::bad(
            "username must be 3-32 characters of a-z, 0-9, '.', '_', '-' and start with a letter or digit",
        ));
    }
    Ok(u)
}
