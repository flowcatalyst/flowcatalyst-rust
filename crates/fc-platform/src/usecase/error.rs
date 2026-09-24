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

/// The category of a [`UseCaseError`]; decides the HTTP status.
///
/// The serde names are the variant names of the enum `UseCaseError` used
/// to be, so the serialized form (`{"type": "ValidationError", ...}`) is
/// unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorKind {
    /// Input validation failed (missing required fields, invalid format,
    /// etc.). HTTP 400.
    #[serde(rename = "ValidationError")]
    Validation,
    /// Business rule violation (entity in wrong state, constraint
    /// violated, etc.). HTTP 409.
    #[serde(rename = "BusinessRuleViolation")]
    BusinessRule,
    /// Entity not found. HTTP 404.
    #[serde(rename = "NotFoundError")]
    NotFound,
    /// Optimistic locking conflict: the entity was modified by another
    /// transaction. HTTP 409.
    #[serde(rename = "ConcurrencyError")]
    Concurrency,
    /// Infrastructure failure: a failed commit ([`UseCaseError::commit`])
    /// or read ([`UseCaseError::internal`]). HTTP 500.
    #[serde(rename = "CommitError")]
    Internal,
}

impl ErrorKind {
    /// The HTTP status code for this kind of error.
    pub fn http_status_code(self) -> u16 {
        match self {
            Self::Validation => 400,
            Self::BusinessRule | Self::Concurrency => 409,
            Self::NotFound => 404,
            Self::Internal => 500,
        }
    }
}

/// A use case failure: a [`ErrorKind`] plus a machine-readable code, a
/// human-readable message and optional structured details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UseCaseError {
    #[serde(rename = "type")]
    kind: ErrorKind,
    code: String,
    message: String,
    #[serde(default)]
    details: HashMap<String, serde_json::Value>,
}

impl UseCaseError {
    fn new(
        kind: ErrorKind,
        code: impl Into<String>,
        message: impl Into<String>,
        details: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            kind,
            code: code.into(),
            message: message.into(),
            details,
        }
    }

    /// Create a validation error with the given code and message.
    pub fn validation(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Validation, code, message, HashMap::new())
    }

    /// Create a validation error with details.
    pub fn validation_with_details(
        code: impl Into<String>,
        message: impl Into<String>,
        details: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self::new(ErrorKind::Validation, code, message, details)
    }

    /// Create a business rule violation error.
    pub fn business_rule(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::BusinessRule, code, message, HashMap::new())
    }

    /// Create a business rule violation with details.
    pub fn business_rule_with_details(
        code: impl Into<String>,
        message: impl Into<String>,
        details: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self::new(ErrorKind::BusinessRule, code, message, details)
    }

    /// Create a not found error.
    pub fn not_found(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, code, message, HashMap::new())
    }

    /// Create a not found error with details.
    pub fn not_found_with_details(
        code: impl Into<String>,
        message: impl Into<String>,
        details: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self::new(ErrorKind::NotFound, code, message, details)
    }

    /// Create a concurrency error.
    pub fn concurrency(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Concurrency, code, message, HashMap::new())
    }

    /// Create a commit error (code `COMMIT_FAILED`).
    pub fn commit(message: impl Into<String>) -> Self {
        Self::new(
            ErrorKind::Internal,
            "COMMIT_FAILED",
            message,
            HashMap::new(),
        )
    }

    /// Create an internal (infrastructure) error: a failed read, a
    /// serialization failure, anything that is not the caller's fault and
    /// not a failed commit. Maps to HTTP 500.
    pub fn internal(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, code, message, HashMap::new())
    }

    /// The error's category.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Get the error code.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Get the error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Structured details (not part of the HTTP body today).
    pub fn details(&self) -> &HashMap<String, serde_json::Value> {
        &self.details
    }

    /// Get the suggested HTTP status code for this error.
    pub fn http_status_code(&self) -> u16 {
        self.kind.http_status_code()
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

impl From<UseCaseError> for PlatformError {
    fn from(err: UseCaseError) -> Self {
        let UseCaseError {
            kind,
            code,
            message,
            ..
        } = err;
        match kind {
            ErrorKind::Validation => PlatformError::Validation {
                message: format!("{}: {}", code, message),
            },
            ErrorKind::BusinessRule => PlatformError::BusinessRule { code, message },
            ErrorKind::NotFound => PlatformError::NotFound {
                entity_type: code,
                id: message,
            },
            ErrorKind::Concurrency => PlatformError::Concurrency { code, message },
            ErrorKind::Internal => PlatformError::Internal {
                message: format!("{}: {}", code, message),
            },
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

        assert_eq!(err.kind(), ErrorKind::BusinessRule);
        assert!(err.details().contains_key("email"));
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

    /// The HTTP response for each kind of use-case error. Pinned so the
    /// shape of `UseCaseError` can change without changing the wire.
    #[tokio::test]
    async fn test_use_case_error_http_responses() {
        use serde_json::json;
        let cases = vec![
            (
                UseCaseError::validation("NAME_REQUIRED", "Name is required"),
                400,
                json!({"error": "VALIDATION_ERROR", "message": "Validation error: NAME_REQUIRED: Name is required"}),
            ),
            (
                UseCaseError::validation_with_details("BAD", "bad", details! { "field" => "name" }),
                400,
                json!({"error": "VALIDATION_ERROR", "message": "Validation error: BAD: bad"}),
            ),
            (
                UseCaseError::business_rule("ROLE_IN_USE", "in use"),
                409,
                json!({"error": "ROLE_IN_USE", "message": "in use"}),
            ),
            (
                UseCaseError::not_found("ROLE_NOT_FOUND", "Role 'r1' not found"),
                404,
                json!({"error": "NOT_FOUND", "message": "Entity not found: ROLE_NOT_FOUND with id Role 'r1' not found"}),
            ),
            (
                UseCaseError::concurrency("STALE", "stale"),
                409,
                json!({"error": "STALE", "message": "stale"}),
            ),
            (
                UseCaseError::commit("tx failed"),
                500,
                // The cause is logged, never sent (Go's shape).
                json!({"error": "INTERNAL_ERROR", "message": "Internal server error"}),
            ),
        ];
        for (err, status, body) in cases {
            assert_eq!(err.http_status_code(), status);
            assert_eq!(render(err.into()).await, (status, body));
        }
    }

    #[test]
    fn test_serialized_form() {
        let err = UseCaseError::validation("CODE", "msg");
        assert_eq!(
            serde_json::to_string(&err).unwrap(),
            r#"{"type":"ValidationError","code":"CODE","message":"msg","details":{}}"#
        );
        let back: UseCaseError =
            serde_json::from_str(r#"{"type":"CommitError","code":"COMMIT_FAILED","message":"m"}"#)
                .unwrap();
        assert_eq!(back.code(), "COMMIT_FAILED");
        assert_eq!(back.http_status_code(), 500);
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
        assert_eq!(err.kind(), ErrorKind::BusinessRule);
        assert_eq!(
            err.details().get("email"),
            Some(&serde_json::json!("duplicate@example.com"))
        );
    }
}
