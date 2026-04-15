//! Password Authentication Service
//!
//! Secure password hashing using Argon2id.

use argon2::{
    password_hash::{
        rand_core::OsRng,
        PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
    },
    Argon2, Algorithm, Params, Version,
};
use tracing::{debug, warn};

use crate::shared::error::{PlatformError, Result};

/// Password policy configuration
#[derive(Debug, Clone)]
pub struct PasswordPolicy {
    /// Minimum password length
    pub min_length: usize,
    /// Maximum password length
    pub max_length: usize,
    /// Require at least one uppercase letter
    pub require_uppercase: bool,
    /// Require at least one lowercase letter
    pub require_lowercase: bool,
    /// Require at least one digit
    pub require_digit: bool,
    /// Require at least one special character
    pub require_special: bool,
    /// Special characters that satisfy the requirement
    pub special_chars: String,
}

impl Default for PasswordPolicy {
    fn default() -> Self {
        Self {
            min_length: 8,
            max_length: 128,
            require_uppercase: true,
            require_lowercase: true,
            require_digit: true,
            require_special: true,
            special_chars: "!@#$%^&*()_+-=[]{}|;':\",./<>?`~".to_string(),
        }
    }
}

impl PasswordPolicy {
    /// Validate a password against the policy
    pub fn validate(&self, password: &str) -> std::result::Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if password.len() < self.min_length {
            errors.push(format!("Password must be at least {} characters", self.min_length));
        }

        if password.len() > self.max_length {
            errors.push(format!("Password must be at most {} characters", self.max_length));
        }

        if self.require_uppercase && !password.chars().any(|c| c.is_ascii_uppercase()) {
            errors.push("Password must contain at least one uppercase letter".to_string());
        }

        if self.require_lowercase && !password.chars().any(|c| c.is_ascii_lowercase()) {
            errors.push("Password must contain at least one lowercase letter".to_string());
        }

        if self.require_digit && !password.chars().any(|c| c.is_ascii_digit()) {
            errors.push("Password must contain at least one digit".to_string());
        }

        if self.require_special && !password.chars().any(|c| self.special_chars.contains(c)) {
            errors.push("Password must contain at least one special character".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Less strict policy for development/testing
    pub fn lenient() -> Self {
        Self {
            min_length: 8,
            max_length: 128,
            require_uppercase: false,
            require_lowercase: false,
            require_digit: false,
            require_special: false,
            special_chars: String::new(),
        }
    }

    /// Relaxed policy used when an SDK caller opts out of complexity enforcement.
    /// Only enforces a non-empty password with a minimal floor — callers own their own rules.
    pub fn relaxed() -> Self {
        Self {
            min_length: 2,
            max_length: 128,
            require_uppercase: false,
            require_lowercase: false,
            require_digit: false,
            require_special: false,
            special_chars: String::new(),
        }
    }
}

/// Argon2id configuration
#[derive(Debug, Clone)]
pub struct Argon2Config {
    /// Memory cost in KiB (default: 65536 = 64 MiB)
    pub memory_cost: u32,
    /// Time cost (iterations) (default: 3)
    pub time_cost: u32,
    /// Parallelism (default: 4)
    pub parallelism: u32,
    /// Output hash length in bytes (default: 32)
    pub output_len: usize,
}

impl Default for Argon2Config {
    fn default() -> Self {
        Self {
            memory_cost: 65536, // 64 MiB
            time_cost: 3,
            parallelism: 4,
            output_len: 32,
        }
    }
}

impl Argon2Config {
    /// Low memory config for testing (faster but less secure)
    pub fn testing() -> Self {
        Self {
            memory_cost: 4096, // 4 MiB
            time_cost: 1,
            parallelism: 1,
            output_len: 32,
        }
    }

    fn to_params(&self) -> Params {
        Params::new(
            self.memory_cost,
            self.time_cost,
            self.parallelism,
            Some(self.output_len),
        )
        .expect("Invalid Argon2 params")
    }
}

/// Password authentication service
pub struct PasswordService {
    argon2: Argon2<'static>,
    policy: PasswordPolicy,
}

impl PasswordService {
    pub fn new(config: Argon2Config, policy: PasswordPolicy) -> Self {
        let params = config.to_params();
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

        Self { argon2, policy }
    }

    /// Hash a password using Argon2id
    pub fn hash_password(&self, password: &str) -> Result<String> {
        self.hash_with_policy(password, &self.policy)
    }

    /// Hash a password, optionally bypassing the configured complexity policy
    /// for SDK callers that enforce their own rules.
    pub fn hash_password_with_complexity(&self, password: &str, enforce_complexity: bool) -> Result<String> {
        if enforce_complexity {
            self.hash_with_policy(password, &self.policy)
        } else {
            self.hash_with_policy(password, &PasswordPolicy::relaxed())
        }
    }

    fn hash_with_policy(&self, password: &str, policy: &PasswordPolicy) -> Result<String> {
        if let Err(errors) = policy.validate(password) {
            return Err(PlatformError::Validation {
                message: errors.join("; "),
            });
        }

        let salt = SaltString::generate(&mut OsRng);

        let hash = self
            .argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| PlatformError::Internal {
                message: format!("Failed to hash password: {}", e),
            })?;

        debug!("Password hashed successfully");
        Ok(hash.to_string())
    }

    /// Verify a password against a stored hash
    pub fn verify_password(&self, password: &str, hash: &str) -> Result<bool> {
        let parsed_hash = PasswordHash::new(hash).map_err(|e| PlatformError::Internal {
            message: format!("Invalid password hash format: {}", e),
        })?;

        match self.argon2.verify_password(password.as_bytes(), &parsed_hash) {
            Ok(()) => {
                debug!("Password verification successful");
                Ok(true)
            }
            Err(argon2::password_hash::Error::Password) => {
                warn!("Password verification failed: incorrect password");
                Ok(false)
            }
            Err(e) => Err(PlatformError::Internal {
                message: format!("Password verification error: {}", e),
            }),
        }
    }

    /// Check if a password hash needs to be upgraded (e.g., if config changed)
    pub fn needs_rehash(&self, hash: &str) -> bool {
        if let Ok(parsed) = PasswordHash::new(hash) {
            // Check if algorithm is Argon2id
            if parsed.algorithm != argon2::Algorithm::Argon2id.ident() {
                return true;
            }

            // Check params (would need to parse and compare)
            // For simplicity, we'll just return false here
            // In production, you'd compare the params in the hash

            false
        } else {
            true // Invalid hash format needs rehash
        }
    }

    /// Validate password against policy without hashing
    pub fn validate_password(&self, password: &str) -> Result<()> {
        self.policy.validate(password).map_err(|errors| {
            PlatformError::Validation {
                message: errors.join("; "),
            }
        })
    }

    /// Validate password, optionally bypassing complexity for SDK callers.
    pub fn validate_password_with_complexity(&self, password: &str, enforce_complexity: bool) -> Result<()> {
        let policy = if enforce_complexity {
            &self.policy
        } else {
            return PasswordPolicy::relaxed().validate(password).map_err(|errors| {
                PlatformError::Validation { message: errors.join("; ") }
            });
        };
        policy.validate(password).map_err(|errors| {
            PlatformError::Validation { message: errors.join("; ") }
        })
    }

    /// Get the current password policy
    pub fn policy(&self) -> &PasswordPolicy {
        &self.policy
    }
}

impl Default for PasswordService {
    fn default() -> Self {
        Self::new(Argon2Config::default(), PasswordPolicy::default())
    }
}

/// Password reset token
#[derive(Debug, Clone)]
pub struct PasswordResetToken {
    pub token: String,
    pub principal_id: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl PasswordResetToken {
    pub fn new(principal_id: impl Into<String>, validity_hours: i64) -> Self {
        use chrono::Utc;

        // Generate secure random token
        let mut token_bytes = [0u8; 32];
        use argon2::password_hash::rand_core::RngCore;
        OsRng.fill_bytes(&mut token_bytes);
        let token = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, token_bytes);

        Self {
            token,
            principal_id: principal_id.into(),
            expires_at: Utc::now() + chrono::Duration::hours(validity_hours),
        }
    }

    pub fn is_expired(&self) -> bool {
        chrono::Utc::now() > self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_policy_default() {
        let policy = PasswordPolicy::default();

        // Valid password
        assert!(policy.validate("SecureP@ss123!").is_ok());

        // Too short
        assert!(policy.validate("Short1!").is_err());

        // No uppercase
        assert!(policy.validate("nouppercase123!").is_err());

        // No lowercase
        assert!(policy.validate("NOLOWERCASE123!").is_err());

        // No digit
        assert!(policy.validate("NoDigits!@#$").is_err());

        // No special char
        assert!(policy.validate("NoSpecialChars123").is_err());
    }

    #[test]
    fn test_password_policy_lenient() {
        let policy = PasswordPolicy::lenient();

        // Simple password works
        assert!(policy.validate("simplepassword").is_ok());

        // Too short still fails
        assert!(policy.validate("short").is_err());
    }

    #[test]
    fn test_hash_and_verify() {
        let service = PasswordService::new(
            Argon2Config::testing(),
            PasswordPolicy::lenient(),
        );

        let password = "testpassword123";
        let hash = service.hash_password(password).unwrap();

        // Hash is PHC format
        assert!(hash.starts_with("$argon2id$"));

        // Verify correct password
        assert!(service.verify_password(password, &hash).unwrap());

        // Verify wrong password
        assert!(!service.verify_password("wrongpassword", &hash).unwrap());
    }

    #[test]
    fn test_hash_uniqueness() {
        let service = PasswordService::new(
            Argon2Config::testing(),
            PasswordPolicy::lenient(),
        );

        let password = "testpassword123";
        let hash1 = service.hash_password(password).unwrap();
        let hash2 = service.hash_password(password).unwrap();

        // Same password produces different hashes (due to random salt)
        assert_ne!(hash1, hash2);

        // But both verify correctly
        assert!(service.verify_password(password, &hash1).unwrap());
        assert!(service.verify_password(password, &hash2).unwrap());
    }

    #[test]
    fn test_password_reset_token() {
        let token = PasswordResetToken::new("principal-123", 24);

        assert_eq!(token.principal_id, "principal-123");
        assert!(!token.is_expired());
        assert!(!token.token.is_empty());
    }

    #[test]
    fn test_password_reset_token_expiry_duration() {
        let before = chrono::Utc::now();
        let token = PasswordResetToken::new("prn_test", 1); // 1 hour
        let after = chrono::Utc::now();

        // expires_at should be ~1 hour from now
        let diff_from_before = (token.expires_at - before).num_seconds();
        let diff_from_after = (token.expires_at - after).num_seconds();
        assert!(diff_from_before >= 3599 && diff_from_before <= 3601);
        assert!(diff_from_after >= 3599 && diff_from_after <= 3601);
    }

    #[test]
    fn test_password_reset_token_zero_hours_is_immediately_expired() {
        let token = PasswordResetToken::new("prn_test", 0);
        // With 0 hours, expires_at == creation time, so should be expired
        // (or very nearly so — allow a tiny margin)
        assert!(token.is_expired() || token.expires_at <= chrono::Utc::now());
    }

    #[test]
    fn test_empty_password_fails_default_policy() {
        let service = PasswordService::new(
            Argon2Config::testing(),
            PasswordPolicy::default(),
        );
        let result = service.hash_password("");
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("at least"), "Expected length error, got: {err_msg}");
    }

    #[test]
    fn test_empty_password_fails_lenient_policy() {
        let service = PasswordService::new(
            Argon2Config::testing(),
            PasswordPolicy::lenient(),
        );
        let result = service.hash_password("");
        assert!(result.is_err());
    }

    #[test]
    fn test_password_too_long_fails() {
        let service = PasswordService::new(
            Argon2Config::testing(),
            PasswordPolicy::default(),
        );
        let long_password = "A".repeat(200) + "a1!";
        let result = service.hash_password(&long_password);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("at most"), "Expected max length error, got: {err_msg}");
    }

    #[test]
    fn test_verify_wrong_password_returns_false() {
        let service = PasswordService::new(
            Argon2Config::testing(),
            PasswordPolicy::lenient(),
        );
        let hash = service.hash_password("correctpassword").unwrap();
        let result = service.verify_password("wrongpassword", &hash).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_verify_empty_password_returns_false() {
        let service = PasswordService::new(
            Argon2Config::testing(),
            PasswordPolicy::lenient(),
        );
        let hash = service.hash_password("realpassword").unwrap();
        let result = service.verify_password("", &hash).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_verify_against_invalid_hash_format_returns_error() {
        let service = PasswordService::new(
            Argon2Config::testing(),
            PasswordPolicy::lenient(),
        );
        let result = service.verify_password("anything", "not-a-valid-hash");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_password_standalone() {
        let service = PasswordService::new(
            Argon2Config::testing(),
            PasswordPolicy::default(),
        );
        // Valid
        assert!(service.validate_password("SecureP@ss123!").is_ok());
        // Invalid (too short)
        assert!(service.validate_password("Sh0!").is_err());
    }

    #[test]
    fn test_needs_rehash_valid_argon2id_hash() {
        let service = PasswordService::new(
            Argon2Config::testing(),
            PasswordPolicy::lenient(),
        );
        let hash = service.hash_password("testpassword").unwrap();
        assert!(!service.needs_rehash(&hash));
    }

    #[test]
    fn test_needs_rehash_garbage_input() {
        let service = PasswordService::new(
            Argon2Config::testing(),
            PasswordPolicy::lenient(),
        );
        assert!(service.needs_rehash("not-a-hash"));
    }

    #[test]
    fn test_password_policy_multiple_errors() {
        let policy = PasswordPolicy::default();
        let result = policy.validate(""); // empty: fails length + all requirements
        assert!(result.is_err());
        let errors = result.unwrap_err();
        // Should report at least length + uppercase + lowercase + digit + special
        assert!(errors.len() >= 5, "Expected >=5 errors, got {}: {:?}", errors.len(), errors);
    }
}
