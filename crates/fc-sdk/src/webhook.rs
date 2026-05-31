//! Webhook signature validation.
//!
//! Validates incoming webhook requests from the FlowCatalyst platform using
//! HMAC-SHA256 signatures.
//!
//! # Example
//!
//! ```ignore
//! use fc_sdk::webhook::WebhookValidator;
//!
//! let validator = WebhookValidator::new("your-signing-secret");
//!
//! // In your webhook handler:
//! let signature = request.header("X-FlowCatalyst-Signature");
//! let timestamp = request.header("X-FlowCatalyst-Timestamp");
//! let body = request.body();
//!
//! validator.validate(signature, timestamp, body)?;
//! ```

use hmac::{Hmac, Mac};
use sha2::Sha256;

/// Header name for the HMAC-SHA256 signature (hex-encoded).
pub const SIGNATURE_HEADER: &str = "X-FlowCatalyst-Signature";

/// Header name for the timestamp. The FlowCatalyst router emits an ISO8601
/// value with millisecond precision (e.g. `2026-05-24T08:30:00.123Z`). HTTP
/// header lookups are case-insensitive, so this matches the router's
/// uppercase `X-FLOWCATALYST-TIMESTAMP` in any compliant framework.
pub const TIMESTAMP_HEADER: &str = "X-FlowCatalyst-Timestamp";

/// Default timestamp tolerance in seconds (5 minutes).
pub const DEFAULT_TOLERANCE_SECS: u64 = 300;

/// Future clock-skew grace period in seconds.
const FUTURE_GRACE_SECS: u64 = 60;

type HmacSha256 = Hmac<Sha256>;

/// Validates HMAC-SHA256 webhook signatures from FlowCatalyst.
#[derive(Clone)]
pub struct WebhookValidator {
    secret: Vec<u8>,
    tolerance_secs: u64,
}

/// Errors that can occur during webhook validation.
#[derive(Debug, thiserror::Error)]
pub enum WebhookValidationError {
    #[error("missing signature header ({SIGNATURE_HEADER})")]
    MissingSignature,

    #[error("missing timestamp header ({TIMESTAMP_HEADER})")]
    MissingTimestamp,

    #[error("invalid timestamp")]
    InvalidTimestamp,

    #[error("invalid signature")]
    InvalidSignature,

    #[error("timestamp expired (tolerance: {tolerance_secs}s)")]
    TimestampExpired { tolerance_secs: u64 },

    #[error("timestamp is in the future")]
    TimestampInFuture,

    #[error("signing secret not configured")]
    MissingSecret,
}

impl WebhookValidator {
    /// Create a new validator with the given signing secret.
    pub fn new(secret: impl AsRef<[u8]>) -> Self {
        Self {
            secret: secret.as_ref().to_vec(),
            tolerance_secs: DEFAULT_TOLERANCE_SECS,
        }
    }

    /// Create a validator from the `FLOWCATALYST_SIGNING_SECRET` environment variable.
    ///
    /// Returns `Err(MissingSecret)` if the variable is not set or empty.
    pub fn from_env() -> Result<Self, WebhookValidationError> {
        let secret = std::env::var("FLOWCATALYST_SIGNING_SECRET")
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or(WebhookValidationError::MissingSecret)?;
        Ok(Self::new(secret))
    }

    /// Set a custom timestamp tolerance (default: 300 seconds).
    pub fn with_tolerance(mut self, seconds: u64) -> Self {
        self.tolerance_secs = seconds;
        self
    }

    /// Validate a webhook request.
    ///
    /// - `signature` — value of the `X-FlowCatalyst-Signature` header
    /// - `timestamp` — value of the `X-FlowCatalyst-Timestamp` header (ISO8601
    ///   with millisecond precision, e.g. `2026-05-24T08:30:00.123Z`; a bare
    ///   Unix-seconds integer is also accepted for backward compatibility)
    /// - `payload` — raw request body bytes
    pub fn validate(
        &self,
        signature: Option<&str>,
        timestamp: Option<&str>,
        payload: &[u8],
    ) -> Result<(), WebhookValidationError> {
        let signature = signature.ok_or(WebhookValidationError::MissingSignature)?;
        let timestamp_str = timestamp.ok_or(WebhookValidationError::MissingTimestamp)?;

        let webhook_time =
            parse_timestamp(timestamp_str).ok_or(WebhookValidationError::InvalidTimestamp)?;

        // Validate timestamp freshness
        self.validate_timestamp(webhook_time)?;

        // Compute expected signature: HMAC-SHA256(timestamp + payload)
        let expected = self.compute_signature(timestamp_str, payload);

        // Constant-time comparison
        if !constant_time_eq(signature.as_bytes(), expected.as_bytes()) {
            return Err(WebhookValidationError::InvalidSignature);
        }

        Ok(())
    }

    /// Compute the expected HMAC-SHA256 signature for a given timestamp and payload.
    ///
    /// The signed message is `{timestamp}{payload}` (concatenated, no separator).
    pub fn compute_signature(&self, timestamp: &str, payload: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key size");
        mac.update(timestamp.as_bytes());
        mac.update(payload);
        hex::encode(mac.finalize().into_bytes())
    }

    fn validate_timestamp(&self, webhook_time: u64) -> Result<(), WebhookValidationError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Too old?
        if webhook_time < now.saturating_sub(self.tolerance_secs) {
            return Err(WebhookValidationError::TimestampExpired {
                tolerance_secs: self.tolerance_secs,
            });
        }

        // Too far in the future?
        if webhook_time > now + FUTURE_GRACE_SECS {
            return Err(WebhookValidationError::TimestampInFuture);
        }

        Ok(())
    }
}

/// Parse the `X-FlowCatalyst-Timestamp` value into Unix seconds.
///
/// The FlowCatalyst router emits ISO8601 with millisecond precision (e.g.
/// `2026-05-24T08:30:00.123Z`); we accept any RFC3339 timestamp and, for
/// backward compatibility, a bare Unix-seconds integer.
fn parse_timestamp(s: &str) -> Option<u64> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        let secs = dt.timestamp();
        if secs >= 0 {
            return Some(secs as u64);
        }
    }
    s.parse::<u64>().ok()
}

/// Constant-time byte comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_and_validate_signature() {
        let validator = WebhookValidator::new("test-secret");
        let payload = b"{\"type\":\"order.created\"}";
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let timestamp = now.to_string();

        let sig = validator.compute_signature(&timestamp, payload);

        // Should pass validation
        assert!(validator
            .validate(Some(&sig), Some(&timestamp), payload)
            .is_ok());
    }

    #[test]
    fn test_invalid_signature_rejected() {
        let validator = WebhookValidator::new("test-secret");
        let payload = b"{\"type\":\"order.created\"}";
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let timestamp = now.to_string();

        let result = validator.validate(Some("bad-sig"), Some(&timestamp), payload);
        assert!(matches!(
            result,
            Err(WebhookValidationError::InvalidSignature)
        ));
    }

    #[test]
    fn test_expired_timestamp_rejected() {
        let validator = WebhookValidator::new("test-secret").with_tolerance(60);
        let payload = b"{}";
        let old_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 120; // 2 minutes ago
        let timestamp = old_time.to_string();
        let sig = validator.compute_signature(&timestamp, payload);

        let result = validator.validate(Some(&sig), Some(&timestamp), payload);
        assert!(matches!(
            result,
            Err(WebhookValidationError::TimestampExpired { .. })
        ));
    }

    #[test]
    fn test_missing_headers() {
        let validator = WebhookValidator::new("test-secret");
        assert!(matches!(
            validator.validate(None, Some("123"), b""),
            Err(WebhookValidationError::MissingSignature)
        ));
        assert!(matches!(
            validator.validate(Some("abc"), None, b""),
            Err(WebhookValidationError::MissingTimestamp)
        ));
    }

    #[test]
    fn test_tampered_payload_rejected() {
        let validator = WebhookValidator::new("test-secret");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let timestamp = now.to_string();
        let sig = validator.compute_signature(&timestamp, b"original");

        let result = validator.validate(Some(&sig), Some(&timestamp), b"tampered");
        assert!(matches!(
            result,
            Err(WebhookValidationError::InvalidSignature)
        ));
    }

    #[test]
    fn test_iso8601_millisecond_timestamp_accepted() {
        // The router signs with an ISO8601 millisecond timestamp
        // (e.g. 2026-05-24T08:30:00.123Z); the validator must accept it.
        let validator = WebhookValidator::new("test-secret");
        let payload = b"{\"type\":\"order.created\"}";
        let timestamp = chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        let sig = validator.compute_signature(&timestamp, payload);

        assert!(validator
            .validate(Some(&sig), Some(&timestamp), payload)
            .is_ok());
    }

    #[test]
    fn test_iso8601_expired_timestamp_rejected() {
        let validator = WebhookValidator::new("test-secret").with_tolerance(60);
        let payload = b"{}";
        let old = chrono::Utc::now() - chrono::Duration::seconds(120);
        let timestamp = old.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        let sig = validator.compute_signature(&timestamp, payload);

        assert!(matches!(
            validator.validate(Some(&sig), Some(&timestamp), payload),
            Err(WebhookValidationError::TimestampExpired { .. })
        ));
    }
}
