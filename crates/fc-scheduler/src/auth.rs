//! Dispatch Auth Service
//!
//! Generates and validates HMAC-SHA256 auth tokens for dispatch job processing.
//!
//! Authentication flow between platform and message router:
//! 1. Platform creates a dispatch job and generates an HMAC token using the app key
//! 2. Platform sends the job to SQS with the token in the MessagePointer
//! 3. Message router receives the message and calls back to platform with the same token
//! 4. Platform validates the token by re-computing the HMAC and comparing
//!
//! Token is computed as: HMAC-SHA256(dispatchJobId, appKey)

use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("App key is not configured")]
    AppKeyNotConfigured,
    #[error("Invalid auth token")]
    InvalidToken,
}

/// Dispatch authentication service for generating and validating HMAC tokens
#[derive(Clone)]
pub struct DispatchAuthService {
    app_key: Option<String>,
}

impl DispatchAuthService {
    /// Create a new dispatch auth service with the given app key
    pub fn new(app_key: Option<String>) -> Self {
        Self { app_key }
    }

    /// Generate an HMAC-SHA256 auth token for a dispatch job ID
    /// Returns the hex-encoded token
    pub fn generate_auth_token(&self, dispatch_job_id: &str) -> Result<String, AuthError> {
        let key = self.app_key.as_ref()
            .ok_or(AuthError::AppKeyNotConfigured)?;

        if key.is_empty() {
            return Err(AuthError::AppKeyNotConfigured);
        }

        Ok(self.hmac_sha256_hex(dispatch_job_id, key))
    }

    /// Validate an auth token from the message router
    pub fn validate_auth_token(&self, dispatch_job_id: &str, token: &str) -> Result<(), AuthError> {
        if token.is_empty() || dispatch_job_id.is_empty() {
            return Err(AuthError::InvalidToken);
        }

        let expected = self.generate_auth_token(dispatch_job_id)?;

        // Use constant-time comparison to prevent timing attacks
        if expected.as_bytes().ct_eq(token.as_bytes()).into() {
            Ok(())
        } else {
            Err(AuthError::InvalidToken)
        }
    }

    /// Check if the app key is configured
    pub fn is_configured(&self) -> bool {
        self.app_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false)
    }

    /// Compute HMAC-SHA256 and return hex-encoded result (lowercase)
    fn hmac_sha256_hex(&self, data: &str, secret: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(data.as_bytes());
        let result = mac.finalize();
        hex::encode(result.into_bytes())
    }
}

impl Default for DispatchAuthService {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_validate_token() {
        let service = DispatchAuthService::new(Some("test-secret-key".to_string()));
        let job_id = "0HZXEQ5Y8JY5Z";

        let token = service.generate_auth_token(job_id).unwrap();
        assert!(!token.is_empty());
        assert_eq!(token.len(), 64); // SHA256 produces 32 bytes = 64 hex chars

        // Should validate successfully
        assert!(service.validate_auth_token(job_id, &token).is_ok());

        // Should fail with wrong token
        assert!(service.validate_auth_token(job_id, "wrong-token").is_err());
    }

    #[test]
    fn test_no_app_key() {
        let service = DispatchAuthService::new(None);
        assert!(service.generate_auth_token("job123").is_err());
    }

    #[test]
    fn test_empty_app_key() {
        let service = DispatchAuthService::new(Some(String::new()));
        assert!(service.generate_auth_token("job123").is_err());
        assert!(!service.is_configured());
    }

    #[test]
    fn test_deterministic_tokens() {
        let service = DispatchAuthService::new(Some("secret".to_string()));
        let token1 = service.generate_auth_token("job1").unwrap();
        let token2 = service.generate_auth_token("job1").unwrap();
        assert_eq!(token1, token2);
    }
}
