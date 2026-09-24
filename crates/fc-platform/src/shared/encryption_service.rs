//! Application-level Encryption Service
//!
//! Encrypts/decrypts sensitive field values (OAuth client secrets, webhook signing keys, etc.)
//! using AES-256-GCM with the `FLOWCATALYST_APP_KEY` environment variable.
//!
//! ## Key Rotation
//!
//! Supports seamless key rotation via `FLOWCATALYST_APP_KEY_PREVIOUS`:
//! - New encryptions always use the current key (version 1)
//! - Decryption tries the current key first, falls back to previous key(s)
//! - The encrypted format is `base64(version_byte || nonce || ciphertext)`
//! - Version 0 = legacy (no version byte, for backwards compatibility)
//! - Version 1 = current versioned format
//!
//! ### Rotation procedure:
//! 1. Set `FLOWCATALYST_APP_KEY_PREVIOUS` to the current key
//! 2. Set `FLOWCATALYST_APP_KEY` to a new key (use `EncryptionService::generate_key()`)
//! 3. Restart the server — new data encrypted with new key, old data still decryptable
//! 4. Run re-encryption batch job (`re_encrypt()`) to migrate old data
//! 5. Remove `FLOWCATALYST_APP_KEY_PREVIOUS` after all data is migrated

use aes_gcm::{
    aead::{generic_array::typenum::U12, rand_core::RngCore, Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use std::string::FromUtf8Error;
use tracing::{info, warn};

/// Why a key could not be loaded or a value could not be encrypted or
/// decrypted. The `Display` text is the message these functions used to
/// return as a `String`.
#[derive(Debug, thiserror::Error)]
pub enum EncryptionError {
    #[error("Invalid base64 key: {0}")]
    InvalidKeyEncoding(#[source] base64::DecodeError),
    #[error("Key must be 32 bytes (got {0})")]
    InvalidKeyLength(usize),
    #[error("Encryption failed: {0}")]
    Encrypt(aes_gcm::Error),
    #[error("Invalid base64: {0}")]
    InvalidCiphertextEncoding(#[source] base64::DecodeError),
    #[error("Empty encrypted data")]
    Empty,
    #[error("Encrypted data too short")]
    TooShort,
    #[error("Decrypted data not valid UTF-8: {0}")]
    InvalidUtf8(#[source] FromUtf8Error),
    #[error("Decrypted data not valid UTF-8 (previous key {key}): {source}")]
    InvalidUtf8PreviousKey { key: usize, source: FromUtf8Error },
    #[error("Decryption failed with all available keys")]
    NoKeyMatched,
    #[error("Stored secret is not an `encrypted:` reference")]
    MissingPrefix,
    #[error("FLOWCATALYST_APP_KEY is not configured; secrets cannot be encrypted or decrypted")]
    NotConfigured,
}

/// The service, or [`EncryptionError::NotConfigured`] when no key is set.
/// Secrets are never stored or used in plaintext as a fallback.
pub fn require_configured(
    enc: Option<&EncryptionService>,
) -> Result<&EncryptionService, EncryptionError> {
    enc.ok_or(EncryptionError::NotConfigured)
}

impl From<EncryptionError> for crate::shared::error::PlatformError {
    fn from(e: EncryptionError) -> Self {
        Self::internal(e.to_string())
    }
}

impl From<EncryptionError> for crate::usecase::UseCaseError {
    fn from(e: EncryptionError) -> Self {
        Self::internal("ENCRYPTION_ERROR", e.to_string())
    }
}

/// Marks a stored value as ciphertext from [`EncryptionService`]. Every
/// secret the platform stores carries it; a stored secret without it is
/// plaintext at rest and is refused on read.
pub const ENCRYPTED_PREFIX: &str = "encrypted:";

/// Whether `stored` is an `encrypted:` reference (says nothing about whether
/// it decrypts).
pub fn is_encrypted_ref(stored: &str) -> bool {
    stored.starts_with(ENCRYPTED_PREFIX)
}

/// Current encryption format version.
const CURRENT_VERSION: u8 = 1;

/// Application encryption service for field-level encryption with key rotation support.
#[derive(Clone)]
pub struct EncryptionService {
    /// Current key — used for all new encryptions
    current: Aes256Gcm,
    /// Previous key(s) — used as fallback for decryption during rotation
    previous: Vec<Aes256Gcm>,
}

fn make_cipher(key_base64: &str) -> Result<Aes256Gcm, EncryptionError> {
    let key_bytes = BASE64
        .decode(key_base64)
        .map_err(EncryptionError::InvalidKeyEncoding)?;
    // `new_from_slice` fails only on a wrong key length.
    Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|_| EncryptionError::InvalidKeyLength(key_bytes.len()))
}

impl EncryptionService {
    /// Create with a single key (no rotation).
    pub fn new(key_base64: &str) -> Result<Self, EncryptionError> {
        Ok(Self {
            current: make_cipher(key_base64)?,
            previous: Vec::new(),
        })
    }

    /// Create with current key + previous key(s) for rotation.
    pub fn with_previous_keys(
        current_key: &str,
        previous_keys: &[&str],
    ) -> Result<Self, EncryptionError> {
        let current = make_cipher(current_key)?;
        let previous = previous_keys
            .iter()
            .map(|k| make_cipher(k))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { current, previous })
    }

    /// Create from environment variables.
    /// - `FLOWCATALYST_APP_KEY` — current key (required)
    /// - `FLOWCATALYST_APP_KEY_PREVIOUS` — previous key for rotation (optional)
    pub fn from_env() -> Option<Self> {
        let current_key = std::env::var("FLOWCATALYST_APP_KEY").ok()?;
        let previous_key = std::env::var("FLOWCATALYST_APP_KEY_PREVIOUS").ok();

        let previous_keys: Vec<&str> = previous_key
            .as_deref()
            .filter(|k| !k.is_empty())
            .into_iter()
            .collect();

        match Self::with_previous_keys(&current_key, &previous_keys) {
            Ok(svc) => {
                if previous_keys.is_empty() {
                    info!("Encryption service initialized (FLOWCATALYST_APP_KEY)");
                } else {
                    info!("Encryption service initialized with key rotation support ({} previous key(s))", previous_keys.len());
                }
                Some(svc)
            }
            Err(e) => {
                warn!("Failed to init encryption service: {}", e);
                None
            }
        }
    }

    /// Encrypt a plaintext string using the current key.
    /// Returns base64-encoded `version || nonce || ciphertext`.
    pub fn encrypt(&self, plaintext: &str) -> Result<String, EncryptionError> {
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from(nonce_bytes);

        let ciphertext = self
            .current
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(EncryptionError::Encrypt)?;

        // Versioned format: version_byte || nonce || ciphertext
        let mut output = Vec::with_capacity(1 + 12 + ciphertext.len());
        output.push(CURRENT_VERSION);
        output.extend_from_slice(&nonce_bytes);
        output.extend(ciphertext);
        Ok(BASE64.encode(output))
    }

    /// Encrypt `plaintext` into the stored form: `encrypted:` followed by
    /// [`encrypt`](Self::encrypt)'s output. Use this for every secret
    /// written to the database.
    pub fn encrypt_ref(&self, plaintext: &str) -> Result<String, EncryptionError> {
        Ok(format!("{ENCRYPTED_PREFIX}{}", self.encrypt(plaintext)?))
    }

    /// Decrypt a stored secret written by [`encrypt_ref`](Self::encrypt_ref).
    /// A value without the `encrypted:` prefix is plaintext at rest and is
    /// refused with [`EncryptionError::MissingPrefix`], never passed through.
    pub fn decrypt_ref(&self, stored: &str) -> Result<String, EncryptionError> {
        let raw = stored
            .strip_prefix(ENCRYPTED_PREFIX)
            .ok_or(EncryptionError::MissingPrefix)?;
        self.decrypt(raw)
    }

    /// Decrypt a value. Tries current key first, then falls back to previous keys.
    /// Supports both versioned (v1), legacy (v0), and TypeScript `encrypted:` prefix formats.
    pub fn decrypt(&self, encrypted: &str) -> Result<String, EncryptionError> {
        // Handle TypeScript encryption format: "encrypted:BASE64(iv || ciphertext || tag)"
        let raw = encrypted.strip_prefix("encrypted:").unwrap_or(encrypted);

        let data = BASE64
            .decode(raw)
            .map_err(EncryptionError::InvalidCiphertextEncoding)?;

        if data.is_empty() {
            return Err(EncryptionError::Empty);
        }

        // Check if versioned format (first byte is version)
        if data[0] == CURRENT_VERSION {
            // Versioned: version(1) || nonce(12) || ciphertext
            if data.len() < 14 {
                // 1 + 12 + at least 1 byte ciphertext
                return Err(EncryptionError::TooShort);
            }
            let nonce_bytes: [u8; 12] = data[1..13].try_into().unwrap();
            let nonce = Nonce::from(nonce_bytes);
            let ciphertext = &data[13..];
            return self.try_decrypt_with_fallback(&nonce, ciphertext);
        }

        // Legacy format (v0): nonce(12) || ciphertext (no version byte)
        if data.len() < 13 {
            return Err(EncryptionError::TooShort);
        }
        let nonce_bytes: [u8; 12] = data[..12].try_into().unwrap();
        let nonce = Nonce::from(nonce_bytes);
        let ciphertext = &data[12..];
        self.try_decrypt_with_fallback(&nonce, ciphertext)
    }

    /// Try decrypting with current key, then previous keys.
    fn try_decrypt_with_fallback(
        &self,
        nonce: &Nonce<U12>,
        ciphertext: &[u8],
    ) -> Result<String, EncryptionError> {
        // Try current key first
        if let Ok(plaintext) = self.current.decrypt(nonce, ciphertext) {
            return String::from_utf8(plaintext).map_err(EncryptionError::InvalidUtf8);
        }

        // Try previous keys
        for (i, prev) in self.previous.iter().enumerate() {
            if let Ok(plaintext) = prev.decrypt(nonce, ciphertext) {
                return String::from_utf8(plaintext)
                    .map_err(|source| EncryptionError::InvalidUtf8PreviousKey { key: i, source });
            }
        }

        Err(EncryptionError::NoKeyMatched)
    }

    /// Re-encrypt a value: decrypt with any available key, re-encrypt with current key.
    /// Returns the new encrypted value, or the original if it was already using the current key.
    pub fn re_encrypt(&self, encrypted: &str) -> Result<String, EncryptionError> {
        let plaintext = self.decrypt(encrypted)?;
        self.encrypt(&plaintext)
    }

    /// Check if a value needs re-encryption (encrypted with old key or legacy format).
    pub fn needs_re_encryption(&self, encrypted: &str) -> bool {
        let data = match BASE64.decode(encrypted) {
            Ok(d) => d,
            Err(_) => return false,
        };

        // Legacy format (no version byte) always needs re-encryption
        if data.is_empty() || data[0] != CURRENT_VERSION {
            return true;
        }

        // Versioned format — check if current key can decrypt
        if data.len() < 14 {
            return true;
        }
        let nonce_bytes: [u8; 12] = data[1..13].try_into().unwrap();
        let nonce = Nonce::from(nonce_bytes);
        let ciphertext = &data[13..];
        self.current.decrypt(&nonce, ciphertext).is_err()
    }

    /// Generate a new random 32-byte key, base64-encoded.
    pub fn generate_key() -> String {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        BASE64.encode(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = EncryptionService::generate_key();
        let svc = EncryptionService::new(&key).unwrap();

        let plaintext = "super-secret-oauth-client-secret";
        let encrypted = svc.encrypt(plaintext).unwrap();
        assert_ne!(encrypted, plaintext);

        let decrypted = svc.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_different_nonce_each_time() {
        let key = EncryptionService::generate_key();
        let svc = EncryptionService::new(&key).unwrap();

        let e1 = svc.encrypt("same").unwrap();
        let e2 = svc.encrypt("same").unwrap();
        assert_ne!(e1, e2);
    }

    #[test]
    fn test_invalid_key_length() {
        let short_key = BASE64.encode([0u8; 16]);
        let err = EncryptionService::new(&short_key).err().unwrap();
        assert!(matches!(err, EncryptionError::InvalidKeyLength(16)));
        assert_eq!(err.to_string(), "Key must be 32 bytes (got 16)");
    }

    #[test]
    fn test_decrypt_errors_keep_their_messages() {
        let svc = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        let err = svc.decrypt("").unwrap_err();
        assert!(matches!(err, EncryptionError::Empty));
        assert_eq!(err.to_string(), "Empty encrypted data");

        let err = svc.decrypt("not base64!").unwrap_err();
        assert!(matches!(err, EncryptionError::InvalidCiphertextEncoding(_)));
        assert!(err.to_string().starts_with("Invalid base64: "));

        let other = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        let err = svc.decrypt(&other.encrypt("x").unwrap()).unwrap_err();
        assert!(matches!(err, EncryptionError::NoKeyMatched));
    }

    #[test]
    fn test_encrypt_ref_roundtrip() {
        let svc = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        let stored = svc.encrypt_ref("idp-client-secret").unwrap();
        assert!(stored.starts_with("encrypted:"));
        assert!(is_encrypted_ref(&stored));
        assert!(!stored.contains("idp-client-secret"));
        assert_eq!(svc.decrypt_ref(&stored).unwrap(), "idp-client-secret");
    }

    #[test]
    fn test_decrypt_ref_requires_prefix() {
        let svc = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        // Plaintext at rest is refused, not passed through.
        let err = svc.decrypt_ref("plain-secret").unwrap_err();
        assert!(matches!(err, EncryptionError::MissingPrefix));
        // So is bare ciphertext without the prefix.
        let bare = svc.encrypt("x").unwrap();
        assert!(matches!(
            svc.decrypt_ref(&bare).unwrap_err(),
            EncryptionError::MissingPrefix
        ));
        assert!(!is_encrypted_ref(&bare));
    }

    #[test]
    fn test_require_configured() {
        assert!(matches!(
            require_configured(None).err(),
            Some(EncryptionError::NotConfigured)
        ));
        let svc = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        assert!(require_configured(Some(&svc)).is_ok());
    }

    #[test]
    fn test_decrypt_ref_wrong_key_fails() {
        let svc = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        let other = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        let stored = other.encrypt_ref("x").unwrap();
        assert!(matches!(
            svc.decrypt_ref(&stored).unwrap_err(),
            EncryptionError::NoKeyMatched
        ));
    }

    #[test]
    fn test_key_rotation_decrypt_with_previous() {
        let old_key = EncryptionService::generate_key();
        let new_key = EncryptionService::generate_key();

        // Encrypt with old key
        let old_svc = EncryptionService::new(&old_key).unwrap();
        let encrypted = old_svc.encrypt("secret-data").unwrap();

        // New service with rotation: can decrypt old data
        let new_svc = EncryptionService::with_previous_keys(&new_key, &[&old_key]).unwrap();
        let decrypted = new_svc.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, "secret-data");
    }

    #[test]
    fn test_key_rotation_new_encryptions_use_current() {
        let old_key = EncryptionService::generate_key();
        let new_key = EncryptionService::generate_key();

        let new_svc = EncryptionService::with_previous_keys(&new_key, &[&old_key]).unwrap();
        let encrypted = new_svc.encrypt("new-data").unwrap();

        // Only current key should decrypt new data
        let current_only = EncryptionService::new(&new_key).unwrap();
        assert_eq!(current_only.decrypt(&encrypted).unwrap(), "new-data");

        // Old key alone should NOT decrypt new data
        let old_only = EncryptionService::new(&old_key).unwrap();
        assert!(old_only.decrypt(&encrypted).is_err());
    }

    #[test]
    fn test_re_encrypt_migrates_to_current_key() {
        let old_key = EncryptionService::generate_key();
        let new_key = EncryptionService::generate_key();

        // Encrypt with old key
        let old_svc = EncryptionService::new(&old_key).unwrap();
        let old_encrypted = old_svc.encrypt("migrate-me").unwrap();

        // Re-encrypt with new service
        let new_svc = EncryptionService::with_previous_keys(&new_key, &[&old_key]).unwrap();
        let new_encrypted = new_svc.re_encrypt(&old_encrypted).unwrap();

        // Now decryptable with current key alone
        let current_only = EncryptionService::new(&new_key).unwrap();
        assert_eq!(current_only.decrypt(&new_encrypted).unwrap(), "migrate-me");
    }

    #[test]
    fn test_needs_re_encryption() {
        let old_key = EncryptionService::generate_key();
        let new_key = EncryptionService::generate_key();

        let old_svc = EncryptionService::new(&old_key).unwrap();
        let old_encrypted = old_svc.encrypt("check-me").unwrap();

        let new_svc = EncryptionService::with_previous_keys(&new_key, &[&old_key]).unwrap();

        // Old data needs re-encryption
        assert!(new_svc.needs_re_encryption(&old_encrypted));

        // Freshly encrypted data does not
        let fresh = new_svc.encrypt("fresh").unwrap();
        assert!(!new_svc.needs_re_encryption(&fresh));
    }
}
