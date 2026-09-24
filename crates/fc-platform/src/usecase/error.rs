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

use crate::shared::error::PlatformError;

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

    /// Create an internal (infrastructure) error: a failed read, a
    /// serialization failure, anything that is not the caller's fault and
    /// not a failed commit. Maps to HTTP 500.
    pub fn internal(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::CommitError {
            code: code.into(),
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

/// Lets use-case code apply `?` to repository calls.
///
/// The mapping is chosen so that the HTTP response after the
/// `From<UseCaseError> for PlatformError` round trip keeps the status code
/// of the original `PlatformError`, and for `NotFound`, `BusinessRule`,
/// `Concurrency` and `Duplicate` the whole body. `UseCaseError` has no
/// authentication kinds (those belong to handlers), so `Unauthorized`,
/// `Forbidden` and the token errors become internal errors: a use case
/// never receives them from a repository.
impl From<PlatformError> for UseCaseError {
    fn from(err: PlatformError) -> Self {
        match err {
            PlatformError::NotFound { entity_type, id } => Self::not_found(entity_type, id),
            PlatformError::BusinessRule { code, message } => Self::business_rule(code, message),
            PlatformError::Concurrency { code, message } => Self::concurrency(code, message),
            e @ PlatformError::Duplicate { .. } => Self::business_rule("DUPLICATE", e.to_string()),
            PlatformError::Validation { message } => Self::validation("VALIDATION_ERROR", message),
            e @ (PlatformError::EventTypeNotFound { .. }
            | PlatformError::SubscriptionNotFound { .. }
            | PlatformError::ClientNotFound { .. }
            | PlatformError::PrincipalNotFound { .. }
            | PlatformError::ServiceAccountNotFound { .. }) => {
                Self::not_found("NOT_FOUND", e.to_string())
            }
            PlatformError::Sqlx(e) => Self::internal("DATABASE_ERROR", e.to_string()),
            PlatformError::Internal { message } => Self::internal("INTERNAL_ERROR", message),
            other => Self::internal("INTERNAL_ERROR", other.to_string()),
        }
    }
}

/// `?`-friendly load-or-404 for repository lookups returning
/// `Result<Option<T>, _>`.
///
/// ```ignore
/// let role = self.role_repo.find_by_id(&id).await
///     .or_not_found("ROLE_NOT_FOUND", format!("Role with ID '{}' not found", id))?;
/// ```
///
/// `Ok(None)` becomes [`UseCaseError::not_found`] with the given code and
/// message; `Err(e)` is converted with `From` (a repository error becomes
/// an internal error, not a `COMMIT_FAILED`).
pub trait OrNotFound<T> {
    fn or_not_found(
        self,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<T, UseCaseError>;
}

impl<T, E: Into<UseCaseError>> OrNotFound<T> for Result<Option<T>, E> {
    fn or_not_found(
        self,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<T, UseCaseError> {
        self.map_err(Into::into)?
            .ok_or_else(|| UseCaseError::not_found(code, message))
    }
}

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

    /// HTTP status + JSON body of a `PlatformError` response.
    async fn render(err: PlatformError) -> (u16, serde_json::Value) {
        use axum::response::IntoResponse;
        let resp = err.into_response();
        let status = resp.status().as_u16();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    /// Converting a `PlatformError` into a `UseCaseError` and back must
    /// keep the HTTP response of the original.
    #[tokio::test]
    async fn test_platform_error_round_trip_keeps_response() {
        let cases = || {
            vec![
                PlatformError::not_found("Role", "r1"),
                PlatformError::business_rule("ROLE_IN_USE", "in use"),
                PlatformError::Concurrency {
                    code: "STALE".into(),
                    message: "stale".into(),
                },
                PlatformError::duplicate("Client", "identifier", "acme"),
                PlatformError::conflict("already there"),
            ]
        };
        for (direct, via) in cases().into_iter().zip(cases()) {
            let round_trip: PlatformError = UseCaseError::from(via).into();
            assert_eq!(render(direct).await, render(round_trip).await);
        }
    }

    #[tokio::test]
    async fn test_platform_error_round_trip_keeps_status() {
        let cases = || {
            vec![
                PlatformError::validation("bad"),
                PlatformError::Sqlx(sqlx::Error::RowNotFound),
                PlatformError::internal("boom"),
                PlatformError::EventTypeNotFound {
                    code: "a:b:c".into(),
                },
            ]
        };
        for (direct, via) in cases().into_iter().zip(cases()) {
            let round_trip: PlatformError = UseCaseError::from(via).into();
            assert_eq!(render(direct).await.0, render(round_trip).await.0);
        }
    }

    #[test]
    fn test_database_error_is_internal_not_commit() {
        let err = UseCaseError::from(PlatformError::Sqlx(sqlx::Error::RowNotFound));
        assert_eq!(err.code(), "DATABASE_ERROR");
        assert_eq!(err.http_status_code(), 500);
    }

    #[test]
    fn test_or_not_found() {
        let found: Result<Option<u32>, PlatformError> = Ok(Some(7));
        assert_eq!(found.or_not_found("X_NOT_FOUND", "gone").unwrap(), 7);

        let missing: Result<Option<u32>, PlatformError> = Ok(None);
        let err = missing.or_not_found("X_NOT_FOUND", "gone").unwrap_err();
        assert_eq!(err.code(), "X_NOT_FOUND");
        assert_eq!(err.message(), "gone");
        assert_eq!(err.http_status_code(), 404);

        let failed: Result<Option<u32>, PlatformError> =
            Err(PlatformError::Sqlx(sqlx::Error::PoolTimedOut));
        let err = failed.or_not_found("X_NOT_FOUND", "gone").unwrap_err();
        assert_eq!(err.code(), "DATABASE_ERROR");
        assert_eq!(err.http_status_code(), 500);
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
        assert_eq!(
            details.get("email"),
            Some(&serde_json::json!("user@example.com"))
        );
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
        assert_eq!(
            details.get("email"),
            Some(&serde_json::json!("user@example.com"))
        );
        assert_eq!(
            details.get("clientId"),
            Some(&serde_json::json!("client-123"))
        );
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
            assert_eq!(
                details.get("email"),
                Some(&serde_json::json!("duplicate@example.com"))
            );
        }
    }
}
