//! Use Case Errors
//!
//! Categorized error types for use case failures.
//! Errors are categorized by type to enable consistent HTTP status mapping.
//!
//! # Creating Errors with Details
//!
//! Use the `details!` macro for convenient error creation:
//!
//! ```ignore
//! use fc_platform::usecase::{UseCaseError, details};
//!
//! // Simple error
//! UseCaseError::validation("EMAIL_REQUIRED", "Email is required");
//!
//! // Error with details
//! UseCaseError::validation_with_details(
//!     "EMAIL_EXISTS",
//!     "Email already exists",
//!     details!{ "email" => email },
//! );
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Macro for creating error detail maps.
///
/// # Example
///
/// ```ignore
/// use fc_platform::usecase::details;
///
/// let details = details! {
///     "email" => "user@example.com",
///     "clientId" => client_id
/// };
/// ```
#[macro_export]
macro_rules! details {
    () => {
        std::collections::HashMap::new()
    };
    ($($key:expr => $value:expr),+ $(,)?) => {{
        let mut map = std::collections::HashMap::new();
        $(
            map.insert($key.to_string(), serde_json::json!($value));
        )+
        map
    }};
}

/// Categorized error types for use case failures.
///
/// Each variant maps to a specific HTTP status code:
/// - `ValidationError` -> 400 Bad Request
/// - `BusinessRuleViolation` -> 409 Conflict
/// - `NotFoundError` -> 404 Not Found
/// - `ConcurrencyError` -> 409 Conflict
/// - `CommitError` -> 500 Internal Server Error
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UseCaseError {
    /// Input validation failed (missing required fields, invalid format, etc.)
    /// Maps to HTTP 400 Bad Request.
    ValidationError {
        code: String,
        message: String,
        #[serde(default)]
        details: HashMap<String, serde_json::Value>,
    },

    /// Business rule violation (entity in wrong state, constraint violated, etc.)
    /// Maps to HTTP 409 Conflict.
    BusinessRuleViolation {
        code: String,
        message: String,
        #[serde(default)]
        details: HashMap<String, serde_json::Value>,
    },

    /// Entity not found.
    /// Maps to HTTP 404 Not Found.
    NotFoundError {
        code: String,
        message: String,
        #[serde(default)]
        details: HashMap<String, serde_json::Value>,
    },

    /// Optimistic locking conflict - entity was modified by another transaction.
    /// Maps to HTTP 409 Conflict.
    ConcurrencyError {
        code: String,
        message: String,
        #[serde(default)]
        details: HashMap<String, serde_json::Value>,
    },

    /// Transaction commit failed.
    /// Maps to HTTP 500 Internal Server Error.
    CommitError {
        code: String,
        message: String,
        #[serde(default)]
        details: HashMap<String, serde_json::Value>,
    },
}

impl UseCaseError {
    /// Create a validation error with the given code and message.
    pub fn validation(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ValidationError {
            code: code.into(),
            message: message.into(),
            details: HashMap::new(),
        }
    }

    /// Create a validation error with details.
    pub fn validation_with_details(
        code: impl Into<String>,
        message: impl Into<String>,
        details: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self::ValidationError {
            code: code.into(),
            message: message.into(),
            details,
        }
    }

    /// Create a business rule violation error.
    pub fn business_rule(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::BusinessRuleViolation {
            code: code.into(),
            message: message.into(),
            details: HashMap::new(),
        }
    }

    /// Create a business rule violation with details.
    pub fn business_rule_with_details(
        code: impl Into<String>,
        message: impl Into<String>,
        details: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self::BusinessRuleViolation {
            code: code.into(),
            message: message.into(),
            details,
        }
    }

    /// Create a not found error.
    pub fn not_found(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::NotFoundError {
            code: code.into(),
            message: message.into(),
            details: HashMap::new(),
        }
    }

    /// Create a not found error with details.
    pub fn not_found_with_details(
        code: impl Into<String>,
        message: impl Into<String>,
        details: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self::NotFoundError {
            code: code.into(),
            message: message.into(),
            details,
        }
    }

    /// Create a concurrency error.
    pub fn concurrency(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ConcurrencyError {
            code: code.into(),
            message: message.into(),
            details: HashMap::new(),
        }
    }

    /// Create a commit error.
    pub fn commit(message: impl Into<String>) -> Self {
        Self::CommitError {
            code: "COMMIT_FAILED".to_string(),
            message: message.into(),
            details: HashMap::new(),
        }
    }

    /// Get the error code.
    pub fn code(&self) -> &str {
        match self {
            Self::ValidationError { code, .. } => code,
            Self::BusinessRuleViolation { code, .. } => code,
            Self::NotFoundError { code, .. } => code,
            Self::ConcurrencyError { code, .. } => code,
            Self::CommitError { code, .. } => code,
        }
    }

    /// Get the error message.
    pub fn message(&self) -> &str {
        match self {
            Self::ValidationError { message, .. } => message,
            Self::BusinessRuleViolation { message, .. } => message,
            Self::NotFoundError { message, .. } => message,
            Self::ConcurrencyError { message, .. } => message,
            Self::CommitError { message, .. } => message,
        }
    }

    /// Get the suggested HTTP status code for this error.
    pub fn http_status_code(&self) -> u16 {
        match self {
            Self::ValidationError { .. } => 400,
            Self::BusinessRuleViolation { .. } => 409,
            Self::NotFoundError { .. } => 404,
            Self::ConcurrencyError { .. } => 409,
            Self::CommitError { .. } => 500,
        }
    }
}

impl std::fmt::Display for UseCaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code(), self.message())
    }
}

impl std::error::Error for UseCaseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_error() {
        let err = UseCaseError::validation("EMAIL_REQUIRED", "Email is required");
        assert_eq!(err.code(), "EMAIL_REQUIRED");
        assert_eq!(err.message(), "Email is required");
        assert_eq!(err.http_status_code(), 400);
    }

    #[test]
    fn test_not_found_error() {
        let err = UseCaseError::not_found("USER_NOT_FOUND", "User not found");
        assert_eq!(err.http_status_code(), 404);
    }

    #[test]
    fn test_business_rule_with_details() {
        let mut details = HashMap::new();
        details.insert("email".to_string(), serde_json::json!("test@example.com"));

        let err = UseCaseError::business_rule_with_details(
            "EMAIL_EXISTS",
            "Email already exists",
            details,
        );

        if let UseCaseError::BusinessRuleViolation { details, .. } = err {
            assert!(details.contains_key("email"));
        } else {
            panic!("Expected BusinessRuleViolation");
        }
    }

    #[test]
    fn test_details_macro_empty() {
        let details: HashMap<String, serde_json::Value> = details!();
        assert!(details.is_empty());
    }

    #[test]
    fn test_details_macro_single() {
        let email = "user@example.com";
        let details = details! { "email" => email };
        assert_eq!(details.get("email"), Some(&serde_json::json!("user@example.com")));
    }

    #[test]
    fn test_details_macro_multiple() {
        let email = "user@example.com";
        let client_id = "client-123";
        let details = details! {
            "email" => email,
            "clientId" => client_id,
            "count" => 42,
        };
        assert_eq!(details.get("email"), Some(&serde_json::json!("user@example.com")));
        assert_eq!(details.get("clientId"), Some(&serde_json::json!("client-123")));
        assert_eq!(details.get("count"), Some(&serde_json::json!(42)));
    }

    #[test]
    fn test_details_macro_with_error() {
        let email = "duplicate@example.com";
        let err = UseCaseError::business_rule_with_details(
            "EMAIL_EXISTS",
            format!("Email '{}' already exists", email),
            details! { "email" => email },
        );

        assert_eq!(err.code(), "EMAIL_EXISTS");
        if let UseCaseError::BusinessRuleViolation { details, .. } = err {
            assert_eq!(details.get("email"), Some(&serde_json::json!("duplicate@example.com")));
        }
    }
}
