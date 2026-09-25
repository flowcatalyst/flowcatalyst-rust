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
//!
//! ## Verify-only secrets
//!
//! OAuth client secrets are never resent by the platform, only checked, so
//! they are stored as a keyed hash instead of ciphertext:
//! `hashed:v1:` + base64(HMAC-SHA256(current key bytes, plaintext)). This is
//! byte-compatible with the Go platform's `encryption.Service.Hash`, which
//! writes the same table. [`EncryptionService::verify_secret`] accepts that
//! form (current key, then previous keys) and the older reversible forms,
//! and reports when the stored value should be rewritten to the current
//! hashed form.

use aes_gcm::{
    aead::{generic_array::typenum::U12, rand_core::RngCore, Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::string::FromUtf8Error;
use subtle::ConstantTimeEq;
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

/// Marks a verify-only secret stored as a keyed hash (see
/// [`EncryptionService::hash_secret`]). A closed claim: a value with this
/// prefix is only ever checked against its MAC, never decrypted.
pub const HASHED_PREFIX: &str = "hashed:v1:";

/// Marks a value that is its own plaintext. The Go platform's `Decrypt`
/// honours it, so [`EncryptionService::verify_secret`] does too.
const LITERAL_PREFIX: &str = "literal:";

/// Current encryption format version.
const CURRENT_VERSION: u8 = 1;

/// Application encryption service for field-level encryption with key rotation support.
#[derive(Clone)]
pub struct EncryptionService {
    /// Current key — used for all new encryptions
    current: Aes256Gcm,
    /// Previous key(s) — used as fallback for decryption during rotation
    previous: Vec<Aes256Gcm>,
    /// Raw bytes of `current`, the HMAC key for [`Self::hash_secret`].
    current_key: Vec<u8>,
    /// Raw bytes of each `previous` key, in the same order.
    previous_keys: Vec<Vec<u8>>,
}

fn decode_key(key_base64: &str) -> Result<(Aes256Gcm, Vec<u8>), EncryptionError> {
    let key_bytes = BASE64
        .decode(key_base64)
        .map_err(EncryptionError::InvalidKeyEncoding)?;
    // `new_from_slice` fails only on a wrong key length.
    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|_| EncryptionError::InvalidKeyLength(key_bytes.len()))?;
    Ok((cipher, key_bytes))
}

/// HMAC-SHA256(key, plaintext).
fn mac_for(key: &[u8], plaintext: &str) -> Vec<u8> {
    // HMAC accepts a key of any length; this cannot fail.
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(plaintext.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// Constant-time equality; slices of different length compare unequal.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

impl EncryptionService {
    /// Create with a single key (no rotation).
    pub fn new(key_base64: &str) -> Result<Self, EncryptionError> {
        Self::with_previous_keys(key_base64, &[])
    }

    /// Create with current key + previous key(s) for rotation.
    pub fn with_previous_keys(
        current_key: &str,
        previous_keys: &[&str],
    ) -> Result<Self, EncryptionError> {
        let (current, current_key) = decode_key(current_key)?;
        let (previous, previous_keys) = previous_keys
            .iter()
            .map(|k| decode_key(k))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .unzip();
        Ok(Self {
            current,
            previous,
            current_key,
            previous_keys,
        })
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

    /// Go `MustFromEnv` (encryption.go): `Ok(None)` when
    /// `FLOWCATALYST_APP_KEY` is unset — the documented "encryption disabled"
    /// state — and an error when it (or `FLOWCATALYST_APP_KEY_PREVIOUS`) is
    /// set but malformed, which Go treats as a fatal boot misconfiguration
    /// (owner ruling 2026-09-08) rather than carrying on with every secret
    /// read failing.
    pub fn from_env_checked() -> Result<Option<Self>, EncryptionError> {
        let Some(current) = std::env::var("FLOWCATALYST_APP_KEY")
            .ok()
            .filter(|k| !k.is_empty())
        else {
            return Ok(None);
        };
        let previous = std::env::var("FLOWCATALYST_APP_KEY_PREVIOUS")
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());
        let previous: Vec<&str> = previous.as_deref().into_iter().collect();
        Self::with_previous_keys(&current, &previous).map(Some)
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
        // Versioned: version(1) || nonce(12) || at least 1 byte ciphertext
        if data[0] == CURRENT_VERSION && data.len() >= 14 {
            let nonce_bytes: [u8; 12] = data[1..13].try_into().unwrap();
            let nonce = Nonce::from(nonce_bytes);
            if let Ok(plaintext) = self.try_decrypt_with_fallback(&nonce, &data[13..]) {
                return Ok(plaintext);
            }
            // A v0 value whose random nonce starts with 0x01 (1 in 256)
            // reads as v1 and fails under every key. Retry it as v0, as the
            // Go platform does: GCM authenticates, so the wrong layout can
            // only fail, never yield a false plaintext.
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

    /// The stored form of a verify-only secret: `hashed:v1:` followed by
    /// base64(HMAC-SHA256(current key bytes, plaintext)). Deterministic and
    /// irreversible. Byte-identical to the Go platform's
    /// `encryption.Service.Hash` for the same key and plaintext.
    pub fn hash_secret(&self, plaintext: &str) -> String {
        format!(
            "{HASHED_PREFIX}{}",
            BASE64.encode(mac_for(&self.current_key, plaintext))
        )
    }

    /// Check `provided` against a stored verify-only secret. Mirrors the Go
    /// platform's `encryption.Service.VerifySecret`:
    ///
    /// - `hashed:v1:<mac>`: compare the MAC in constant time under the
    ///   current key, then each previous key. Never falls back to decrypt:
    ///   a mismatch is a failure.
    /// - anything else (`encrypted:…`, a bare envelope, `literal:…`):
    ///   decrypt, then compare in constant time.
    ///
    /// Returns `(ok, needs_rehash)`. `needs_rehash` is true when the value
    /// matched but is not yet [`hash_secret`](Self::hash_secret) under the
    /// current key (every legacy-shape match, and a hash only a previous key
    /// verifies); the caller should then store `hash_secret(provided)`.
    pub fn verify_secret(&self, stored: &str, provided: &str) -> (bool, bool) {
        if let Some(raw) = stored.strip_prefix(HASHED_PREFIX) {
            let Ok(mac) = BASE64.decode(raw) else {
                return (false, false);
            };
            if ct_eq(&mac, &mac_for(&self.current_key, provided)) {
                return (true, false);
            }
            if self
                .previous_keys
                .iter()
                .any(|key| ct_eq(&mac, &mac_for(key, provided)))
            {
                return (true, true);
            }
            return (false, false);
        }

        let decrypted = match stored.trim().strip_prefix(LITERAL_PREFIX) {
            Some(literal) => literal.to_string(),
            None => match self.decrypt(stored) {
                Ok(pt) => pt,
                Err(_) => return (false, false),
            },
        };
        if ct_eq(decrypted.as_bytes(), provided.as_bytes()) {
            (true, true)
        } else {
            (false, false)
        }
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

    // ── hash_secret / verify_secret ────────────────────────────────────

    /// Key bytes 0x00..0x1f, base64.
    const GOLDEN_KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

    /// Copied verbatim from the Go platform's `TestGoldenHashVector`
    /// (`internal/platform/shared/encryption/encryption_test.go`): the Rust
    /// hash must be byte-identical, because both write `oauth_clients`.
    #[test]
    fn test_hash_secret_matches_go_golden_vector() {
        let key: Vec<u8> = (0u8..32).collect();
        assert_eq!(BASE64.encode(&key), GOLDEN_KEY);
        let svc = EncryptionService::new(GOLDEN_KEY).unwrap();
        let got = svc.hash_secret("client-secret-golden");
        assert_eq!(
            got,
            "hashed:v1:HhInGB9kwvg6VsfBL0oHER0eslXRAg6GBwoTsRa2D4E="
        );
        assert_eq!(
            svc.verify_secret(&got, "client-secret-golden"),
            (true, false)
        );
    }

    #[test]
    fn test_hash_secret_is_keyed() {
        use sha2::Digest;
        let a = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        let b = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        assert_ne!(a.hash_secret("same"), b.hash_secret("same"));
        let unkeyed = format!("{HASHED_PREFIX}{}", BASE64.encode(Sha256::digest(b"same")));
        assert_ne!(a.hash_secret("same"), unkeyed);
    }

    #[test]
    fn test_verify_secret_hashed_roundtrip_and_wrong_secret() {
        let svc = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        let stored = svc.hash_secret("s3cr3t");
        assert!(stored.starts_with(HASHED_PREFIX));
        assert!(!stored.contains("s3cr3t"));
        assert_eq!(svc.verify_secret(&stored, "s3cr3t"), (true, false));
        assert_eq!(svc.verify_secret(&stored, "not-the-secret"), (false, false));
        assert_eq!(svc.verify_secret(&stored, ""), (false, false));
    }

    #[test]
    fn test_verify_secret_hashed_malformed_fails_closed() {
        let svc = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        assert_eq!(
            svc.verify_secret("hashed:v1:not-valid-base64!!!", "anything"),
            (false, false)
        );
        // A truncated MAC is a length mismatch, not a prefix match.
        let stored = svc.hash_secret("x");
        assert_eq!(
            svc.verify_secret(&stored[..stored.len() - 8], "x"),
            (false, false)
        );
    }

    #[test]
    fn test_verify_secret_hashed_never_falls_back_to_decrypt() {
        // A hashed: prefix in front of real ciphertext of the provided value
        // must still fail: the prefix is a closed claim.
        let svc = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        let ct = svc.encrypt("s3cr3t").unwrap();
        assert_eq!(
            svc.verify_secret(&format!("{HASHED_PREFIX}{ct}"), "s3cr3t"),
            (false, false)
        );
    }

    #[test]
    fn test_verify_secret_hashed_previous_key_needs_rehash() {
        let old_key = EncryptionService::generate_key();
        let new_key = EncryptionService::generate_key();
        let stored = EncryptionService::new(&old_key)
            .unwrap()
            .hash_secret("rotate-me");

        let rotating = EncryptionService::with_previous_keys(&new_key, &[&old_key]).unwrap();
        assert_eq!(rotating.verify_secret(&stored, "rotate-me"), (true, true));

        let rehashed = rotating.hash_secret("rotate-me");
        let current_only = EncryptionService::new(&new_key).unwrap();
        assert_eq!(
            current_only.verify_secret(&rehashed, "rotate-me"),
            (true, false)
        );

        // A key not held at all verifies nothing.
        let unrelated = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        assert_eq!(
            rotating.verify_secret(&unrelated.hash_secret("rotate-me"), "rotate-me"),
            (false, false)
        );
    }

    #[test]
    fn test_verify_secret_legacy_encrypted_needs_rehash() {
        let svc = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        let stored = svc.encrypt_ref("legacy-secret").unwrap();
        assert_eq!(svc.verify_secret(&stored, "legacy-secret"), (true, true));
        assert_eq!(svc.verify_secret(&stored, "legacy-secreT"), (false, false));
    }

    #[test]
    fn test_verify_secret_legacy_bare_envelope_needs_rehash() {
        let svc = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        let bare = svc.encrypt("bare-legacy-secret").unwrap();
        assert_eq!(svc.verify_secret(&bare, "bare-legacy-secret"), (true, true));
    }

    #[test]
    fn test_verify_secret_legacy_previous_key_and_literal() {
        let old_key = EncryptionService::generate_key();
        let stored = EncryptionService::new(&old_key)
            .unwrap()
            .encrypt_ref("old")
            .unwrap();
        let rotating =
            EncryptionService::with_previous_keys(&EncryptionService::generate_key(), &[&old_key])
                .unwrap();
        assert_eq!(rotating.verify_secret(&stored, "old"), (true, true));

        // Go's Decrypt treats `literal:` as its own plaintext.
        assert_eq!(rotating.verify_secret("literal:dev", "dev"), (true, true));
        assert_eq!(
            rotating.verify_secret("literal:dev", "other"),
            (false, false)
        );
    }

    #[test]
    fn test_verify_secret_garbage_fails() {
        let svc = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        assert_eq!(svc.verify_secret("", ""), (false, false));
        assert_eq!(
            svc.verify_secret("plain-secret", "plain-secret"),
            (false, false)
        );
        let other = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        let stored = other.encrypt_ref("x").unwrap();
        assert_eq!(svc.verify_secret(&stored, "x"), (false, false));
    }

    #[test]
    fn test_decrypt_v0_value_whose_nonce_starts_with_version_byte() {
        // Build a v0 envelope (nonce || ciphertext, no version byte) whose
        // nonce begins with 0x01, so it first parses as v1.
        let svc = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        let mut nonce_bytes = [7u8; 12];
        nonce_bytes[0] = CURRENT_VERSION;
        let ct = svc
            .current
            .encrypt(&Nonce::from(nonce_bytes), b"v0-secret".as_ref())
            .unwrap();
        let mut data = nonce_bytes.to_vec();
        data.extend(ct);
        assert_eq!(svc.decrypt(&BASE64.encode(data)).unwrap(), "v0-secret");
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
