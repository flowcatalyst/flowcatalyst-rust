//! Use Case Errors
//!
//! Categorized error types for use case failures.
//! Each variant maps to a specific HTTP status code for consistent API responses.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Macro for creating error detail maps.
///
/// # Examples
///
/// ```
/// use fc_sdk::details;
/// use std::collections::HashMap;
///
/// let empty: HashMap<String, serde_json::Value> = details!();
///
/// let single = details! { "email" => "user@example.com" };
///
/// let multiple = details! {
///     "email" => "user@example.com",
///     "clientId" => "clt_123",
///     "count" => 42,
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
/// - `ValidationError` → 400 Bad Request
/// - `BusinessRuleViolation` → 409 Conflict
/// - `NotFoundError` → 404 Not Found
/// - `ConcurrencyError` → 409 Conflict
/// - `CommitError` → 500 Internal Server Error
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UseCaseError {
    ValidationError {
        code: String,
        message: String,
        #[serde(default)]
        details: HashMap<String, serde_json::Value>,
    },

    BusinessRuleViolation {
        code: String,
        message: String,
        #[serde(default)]
        details: HashMap<String, serde_json::Value>,
    },

    NotFoundError {
        code: String,
        message: String,
        #[serde(default)]
        details: HashMap<String, serde_json::Value>,
    },

    ConcurrencyError {
        code: String,
        message: String,
        #[serde(default)]
        details: HashMap<String, serde_json::Value>,
    },

    CommitError {
        code: String,
        message: String,
        #[serde(default)]
        details: HashMap<String, serde_json::Value>,
    },
}

impl UseCaseError {
    pub fn validation(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ValidationError {
            code: code.into(),
            message: message.into(),
            details: HashMap::new(),
        }
    }

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

    pub fn business_rule(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::BusinessRuleViolation {
            code: code.into(),
            message: message.into(),
            details: HashMap::new(),
        }
    }

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

    pub fn not_found(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::NotFoundError {
            code: code.into(),
            message: message.into(),
            details: HashMap::new(),
        }
    }

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

    pub fn concurrency(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ConcurrencyError {
            code: code.into(),
            message: message.into(),
            details: HashMap::new(),
        }
    }

    pub fn commit(message: impl Into<String>) -> Self {
        Self::CommitError {
            code: "COMMIT_FAILED".to_string(),
            message: message.into(),
            details: HashMap::new(),
        }
    }

    pub fn code(&self) -> &str {
        match self {
            Self::ValidationError { code, .. } => code,
            Self::BusinessRuleViolation { code, .. } => code,
            Self::NotFoundError { code, .. } => code,
            Self::ConcurrencyError { code, .. } => code,
            Self::CommitError { code, .. } => code,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::ValidationError { message, .. } => message,
            Self::BusinessRuleViolation { message, .. } => message,
            Self::NotFoundError { message, .. } => message,
            Self::ConcurrencyError { message, .. } => message,
            Self::CommitError { message, .. } => message,
        }
    }

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
