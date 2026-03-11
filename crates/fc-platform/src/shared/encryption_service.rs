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
    aead::{Aead, KeyInit, OsRng, rand_core::RngCore, generic_array::typenum::U12},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use tracing::{info, warn};

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

fn make_cipher(key_base64: &str) -> Result<Aes256Gcm, String> {
    let key_bytes = BASE64.decode(key_base64)
        .map_err(|e| format!("Invalid base64 key: {}", e))?;
    if key_bytes.len() != 32 {
        return Err(format!("Key must be 32 bytes (got {})", key_bytes.len()));
    }
    Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| format!("Failed to init AES cipher: {}", e))
}

impl EncryptionService {
    /// Create with a single key (no rotation).
    pub fn new(key_base64: &str) -> Result<Self, String> {
        Ok(Self {
            current: make_cipher(key_base64)?,
            previous: Vec::new(),
        })
    }

    /// Create with current key + previous key(s) for rotation.
    pub fn with_previous_keys(
        current_key: &str,
        previous_keys: &[&str],
    ) -> Result<Self, String> {
        let current = make_cipher(current_key)?;
        let previous = previous_keys.iter()
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

        let previous_keys: Vec<&str> = previous_key.as_deref()
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
    pub fn encrypt(&self, plaintext: &str) -> Result<String, String> {
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self.current
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| format!("Encryption failed: {}", e))?;

        // Versioned format: version_byte || nonce || ciphertext
        let mut output = Vec::with_capacity(1 + 12 + ciphertext.len());
        output.push(CURRENT_VERSION);
        output.extend_from_slice(&nonce_bytes);
        output.extend(ciphertext);
        Ok(BASE64.encode(output))
    }

    /// Decrypt a value. Tries current key first, then falls back to previous keys.
    /// Supports both versioned (v1) and legacy (v0) formats.
    pub fn decrypt(&self, encrypted: &str) -> Result<String, String> {
        let data = BASE64.decode(encrypted)
            .map_err(|e| format!("Invalid base64: {}", e))?;

        if data.is_empty() {
            return Err("Empty encrypted data".to_string());
        }

        // Check if versioned format (first byte is version)
        if data[0] == CURRENT_VERSION {
            // Versioned: version(1) || nonce(12) || ciphertext
            if data.len() < 14 { // 1 + 12 + at least 1 byte ciphertext
                return Err("Encrypted data too short".to_string());
            }
            let nonce = Nonce::from_slice(&data[1..13]);
            let ciphertext = &data[13..];
            return self.try_decrypt_with_fallback(nonce, ciphertext);
        }

        // Legacy format (v0): nonce(12) || ciphertext (no version byte)
        if data.len() < 13 {
            return Err("Encrypted data too short".to_string());
        }
        let nonce = Nonce::from_slice(&data[..12]);
        let ciphertext = &data[12..];
        self.try_decrypt_with_fallback(nonce, ciphertext)
    }

    /// Try decrypting with current key, then previous keys.
    fn try_decrypt_with_fallback(&self, nonce: &Nonce<U12>, ciphertext: &[u8]) -> Result<String, String> {
        // Try current key first
        if let Ok(plaintext) = self.current.decrypt(nonce, ciphertext) {
            return String::from_utf8(plaintext)
                .map_err(|e| format!("Decrypted data not valid UTF-8: {}", e));
        }

        // Try previous keys
        for (i, prev) in self.previous.iter().enumerate() {
            if let Ok(plaintext) = prev.decrypt(nonce, ciphertext) {
                return String::from_utf8(plaintext)
                    .map_err(|e| format!("Decrypted data not valid UTF-8 (previous key {}): {}", i, e));
            }
        }

        Err("Decryption failed with all available keys".to_string())
    }

    /// Re-encrypt a value: decrypt with any available key, re-encrypt with current key.
    /// Returns the new encrypted value, or the original if it was already using the current key.
    pub fn re_encrypt(&self, encrypted: &str) -> Result<String, String> {
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
        let nonce = Nonce::from_slice(&data[1..13]);
        let ciphertext = &data[13..];
        self.current.decrypt(nonce, ciphertext).is_err()
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
        assert!(EncryptionService::new(&short_key).is_err());
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
