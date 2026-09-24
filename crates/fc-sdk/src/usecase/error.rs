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
/// - `Validation` → 400 Bad Request
/// - `BusinessRuleViolation` → 409 Conflict
/// - `NotFound` → 404 Not Found
/// - `Concurrency` → 409 Conflict
/// - `Commit` → 500 Internal Server Error
///
/// The serialized `type` tag keeps the original names (`ValidationError`,
/// `NotFoundError`, `ConcurrencyError`, `CommitError`), so the JSON shape is
/// unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UseCaseError {
    #[serde(rename = "ValidationError")]
    Validation {
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

    #[serde(rename = "NotFoundError")]
    NotFound {
        code: String,
        message: String,
        #[serde(default)]
        details: HashMap<String, serde_json::Value>,
    },

    #[serde(rename = "ConcurrencyError")]
    Concurrency {
        code: String,
        message: String,
        #[serde(default)]
        details: HashMap<String, serde_json::Value>,
    },

    #[serde(rename = "CommitError")]
    Commit {
        code: String,
        message: String,
        #[serde(default)]
        details: HashMap<String, serde_json::Value>,
    },
}

impl UseCaseError {
    pub fn validation(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Validation {
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
        Self::Validation {
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
        Self::NotFound {
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
        Self::NotFound {
            code: code.into(),
            message: message.into(),
            details,
        }
    }

    pub fn concurrency(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Concurrency {
            code: code.into(),
            message: message.into(),
            details: HashMap::new(),
        }
    }

    pub fn commit(message: impl Into<String>) -> Self {
        Self::Commit {
            code: "COMMIT_FAILED".to_string(),
            message: message.into(),
            details: HashMap::new(),
        }
    }

    pub fn code(&self) -> &str {
        match self {
            Self::Validation { code, .. } => code,
            Self::BusinessRuleViolation { code, .. } => code,
            Self::NotFound { code, .. } => code,
            Self::Concurrency { code, .. } => code,
            Self::Commit { code, .. } => code,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Validation { message, .. } => message,
            Self::BusinessRuleViolation { message, .. } => message,
            Self::NotFound { message, .. } => message,
            Self::Concurrency { message, .. } => message,
            Self::Commit { message, .. } => message,
        }
    }

    pub fn http_status_code(&self) -> u16 {
        match self {
            Self::Validation { .. } => 400,
            Self::BusinessRuleViolation { .. } => 409,
            Self::NotFound { .. } => 404,
            Self::Concurrency { .. } => 409,
            Self::Commit { .. } => 500,
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

    // ─── Constructor Helpers ────────────────────────────────────────────

    #[test]
    fn validation_error() {
        let err = UseCaseError::validation("INVALID_EMAIL", "Email is invalid");
        assert_eq!(err.code(), "INVALID_EMAIL");
        assert_eq!(err.message(), "Email is invalid");
        assert_eq!(err.http_status_code(), 400);
        assert!(matches!(err, UseCaseError::Validation { .. }));
    }

    #[test]
    fn validation_error_with_details() {
        let details = crate::details! { "field" => "email", "value" => "bad@" };
        let err = UseCaseError::validation_with_details("INVALID", "bad input", details);
        assert_eq!(err.code(), "INVALID");
        assert_eq!(err.http_status_code(), 400);
        if let UseCaseError::Validation { details, .. } = &err {
            assert_eq!(details["field"], "email");
            assert_eq!(details["value"], "bad@");
        } else {
            panic!("expected Validation");
        }
    }

    #[test]
    fn business_rule_violation() {
        let err = UseCaseError::business_rule("DUPLICATE", "Already exists");
        assert_eq!(err.code(), "DUPLICATE");
        assert_eq!(err.message(), "Already exists");
        assert_eq!(err.http_status_code(), 409);
        assert!(matches!(err, UseCaseError::BusinessRuleViolation { .. }));
    }

    #[test]
    fn business_rule_with_details() {
        let details = crate::details! { "existing_id" => "clt_123" };
        let err = UseCaseError::business_rule_with_details("DUP", "duplicate", details);
        assert_eq!(err.http_status_code(), 409);
        if let UseCaseError::BusinessRuleViolation { details, .. } = &err {
            assert_eq!(details["existing_id"], "clt_123");
        } else {
            panic!("expected BusinessRuleViolation");
        }
    }

    #[test]
    fn not_found_error() {
        let err = UseCaseError::not_found("CLIENT_NOT_FOUND", "Client not found");
        assert_eq!(err.code(), "CLIENT_NOT_FOUND");
        assert_eq!(err.message(), "Client not found");
        assert_eq!(err.http_status_code(), 404);
        assert!(matches!(err, UseCaseError::NotFound { .. }));
    }

    #[test]
    fn not_found_with_details() {
        let details = crate::details! { "id" => "clt_missing" };
        let err = UseCaseError::not_found_with_details("NF", "not found", details);
        assert_eq!(err.http_status_code(), 404);
        if let UseCaseError::NotFound { details, .. } = &err {
            assert_eq!(details["id"], "clt_missing");
        } else {
            panic!("expected NotFound");
        }
    }

    #[test]
    fn concurrency_error() {
        let err = UseCaseError::concurrency("STALE", "Stale data");
        assert_eq!(err.code(), "STALE");
        assert_eq!(err.message(), "Stale data");
        assert_eq!(err.http_status_code(), 409);
        assert!(matches!(err, UseCaseError::Concurrency { .. }));
    }

    #[test]
    fn commit_error() {
        let err = UseCaseError::commit("database connection lost");
        assert_eq!(err.code(), "COMMIT_FAILED");
        assert_eq!(err.message(), "database connection lost");
        assert_eq!(err.http_status_code(), 500);
        assert!(matches!(err, UseCaseError::Commit { .. }));
    }

    // ─── Display / Error ────────────────────────────────────────────────

    #[test]
    fn display_format() {
        let err = UseCaseError::validation("CODE", "some message");
        let display = format!("{}", err);
        assert_eq!(display, "[CODE] some message");
    }

    #[test]
    fn implements_std_error() {
        let err = UseCaseError::not_found("NF", "gone");
        let _: &dyn std::error::Error = &err;
    }

    // ─── Serialization ─────────────────────────────────────────────────

    #[test]
    fn serialization_includes_type_tag() {
        let err = UseCaseError::validation("V", "invalid");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["type"], "ValidationError");
        assert_eq!(json["code"], "V");
        assert_eq!(json["message"], "invalid");
    }

    #[test]
    fn serialized_type_tags_are_unchanged_by_variant_renames() {
        let cases = [
            (UseCaseError::validation("C", "m"), "ValidationError"),
            (
                UseCaseError::business_rule("C", "m"),
                "BusinessRuleViolation",
            ),
            (UseCaseError::not_found("C", "m"), "NotFoundError"),
            (UseCaseError::concurrency("C", "m"), "ConcurrencyError"),
            (UseCaseError::commit("m"), "CommitError"),
        ];
        for (err, tag) in cases {
            let json = serde_json::to_value(&err).unwrap();
            assert_eq!(json["type"], tag);
            let back: UseCaseError = serde_json::from_value(json).unwrap();
            assert_eq!(back.code(), err.code());
        }
    }

    #[test]
    fn deserialization_round_trip() {
        let err = UseCaseError::business_rule("BR", "rule violated");
        let json = serde_json::to_string(&err).unwrap();
        let deserialized: UseCaseError = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.code(), "BR");
        assert_eq!(deserialized.message(), "rule violated");
        assert!(matches!(
            deserialized,
            UseCaseError::BusinessRuleViolation { .. }
        ));
    }

    #[test]
    fn serialization_with_details() {
        let details = crate::details! { "count" => 42 };
        let err = UseCaseError::validation_with_details("V", "msg", details);
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["details"]["count"], 42);
    }

    #[test]
    fn serialization_empty_details() {
        let err = UseCaseError::validation("V", "msg");
        let json = serde_json::to_value(&err).unwrap();
        // Empty details may or may not be present; just verify it's not a non-empty object
        if let Some(d) = json.get("details") {
            assert!(d.as_object().unwrap().is_empty());
        }
    }

    // ─── details! macro ─────────────────────────────────────────────────

    #[test]
    fn details_macro_empty() {
        let empty: HashMap<String, serde_json::Value> = crate::details!();
        assert!(empty.is_empty());
    }

    #[test]
    fn details_macro_single() {
        let d = crate::details! { "key" => "value" };
        assert_eq!(d.len(), 1);
        assert_eq!(d["key"], "value");
    }

    #[test]
    fn details_macro_multiple_types() {
        let d = crate::details! {
            "name" => "test",
            "count" => 42,
            "active" => true,
        };
        assert_eq!(d.len(), 3);
        assert_eq!(d["name"], "test");
        assert_eq!(d["count"], 42);
        assert_eq!(d["active"], true);
    }

    // ─── Clone ──────────────────────────────────────────────────────────

    #[test]
    fn use_case_error_is_clone() {
        let err = UseCaseError::validation("V", "msg");
        let cloned = err.clone();
        assert_eq!(cloned.code(), "V");
    }
}
