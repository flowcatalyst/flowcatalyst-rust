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
    /// The caller may not do this (Java `UseCaseError.Authorization`: a
    /// scope or application check made by the use case itself, after it has
    /// loaded what it needs). HTTP 403.
    #[serde(rename = "AuthorizationError")]
    Forbidden,
    /// Optimistic locking conflict: the entity was modified by another
    /// transaction. HTTP 409.
    #[serde(rename = "ConcurrencyError")]
    Concurrency,
    /// Infrastructure failure: a failed commit ([`UseCaseError::commit`])
    /// or read ([`UseCaseError::internal`]). HTTP 500.
    #[serde(rename = "CommitError")]
    Internal,
    /// A well-formed request whose bytes or reference cannot be processed
    /// (Java `ArtifactHttpException`'s 422s: a `platform://` artifact that
    /// names another function, or was never uploaded). HTTP 422.
    #[serde(rename = "UnprocessableError")]
    Unprocessable,
    /// A capability this deployment has not configured (Java
    /// `ArtifactHttpException.storeNotConfigured`). HTTP 503.
    #[serde(rename = "UnavailableError")]
    Unavailable,
    /// The write would change nothing: the resource is already in the
    /// requested state (the same digest published again, an alias promoted
    /// to the version it already names, an active function activated, a
    /// retired version retired). The use case stops before its commit, so
    /// no event and no audit row are written. A handler that knows the
    /// operation answers `200` with the existing resource instead; anything
    /// else renders it as the `409` the operation answered before no-ops
    /// were idempotent, with the same code, so an older caller that
    /// tolerated the 409 keeps working.
    #[serde(rename = "UnchangedError")]
    Unchanged,
    /// The caller's precondition no longer holds (an optimistic
    /// `expectedVersion` / `If-Match` that another write has overtaken).
    /// HTTP 412.
    #[serde(rename = "PreconditionFailedError")]
    PreconditionFailed,
}

impl ErrorKind {
    /// The HTTP status code for this kind of error.
    pub fn http_status_code(self) -> u16 {
        match self {
            Self::Validation => 400,
            Self::BusinessRule | Self::Concurrency | Self::Unchanged => 409,
            Self::NotFound => 404,
            Self::Forbidden => 403,
            Self::Internal => 500,
            Self::Unprocessable => 422,
            Self::Unavailable => 503,
            Self::PreconditionFailed => 412,
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

    /// Create an authorization error (HTTP 403), as Java's
    /// `UseCaseException.authorization(code, message)`.
    pub fn forbidden(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Forbidden, code, message, HashMap::new())
    }

    /// Create an unprocessable error (HTTP 422).
    pub fn unprocessable(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unprocessable, code, message, HashMap::new())
    }

    /// Create an unavailable error (HTTP 503).
    pub fn unavailable(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unavailable, code, message, HashMap::new())
    }

    /// A no-op write ([`ErrorKind::Unchanged`]): `code` is the one the
    /// operation's `409` carried before no-ops became idempotent.
    pub fn unchanged(
        code: impl Into<String>,
        message: impl Into<String>,
        details: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self::new(ErrorKind::Unchanged, code, message, details)
    }

    /// Create a precondition failure (HTTP 412), with details.
    pub fn precondition_failed(
        code: impl Into<String>,
        message: impl Into<String>,
        details: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self::new(ErrorKind::PreconditionFailed, code, message, details)
    }

    /// Whether this is a no-op write rather than a failure.
    pub fn is_unchanged(&self) -> bool {
        self.kind == ErrorKind::Unchanged
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

    /// Structured details; sent as the body's `details` for validation
    /// and not-found errors.
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
/// never receives them from a repository. Its [`ErrorKind::Forbidden`] is
/// for a use case's own reach checks.
impl From<PlatformError> for UseCaseError {
    fn from(err: PlatformError) -> Self {
        match err {
            // Code and message are the direct response's, so the round trip
            // back to a PlatformError renders the same body.
            e @ PlatformError::NotFound { .. } => Self::not_found("NOT_FOUND", e.to_string()),
            PlatformError::BusinessRule { code, message } => Self::business_rule(code, message),
            PlatformError::Concurrency { code, message } => Self::concurrency(code, message),
            e @ PlatformError::Duplicate { .. } => Self::business_rule("DUPLICATE", e.to_string()),
            e @ PlatformError::Validation { .. } => {
                Self::validation("VALIDATION_ERROR", e.to_string())
            }
            PlatformError::Coded {
                status,
                code,
                message,
                details,
            } => match status.as_u16() {
                400 => Self::validation_with_details(code, message, details),
                403 => Self::new(ErrorKind::Forbidden, code, message, details),
                404 => Self::not_found_with_details(code, message, details),
                409 => Self::business_rule_with_details(code, message, details),
                412 => Self::new(ErrorKind::PreconditionFailed, code, message, details),
                422 => Self::new(ErrorKind::Unprocessable, code, message, details),
                503 => Self::new(ErrorKind::Unavailable, code, message, details),
                _ => Self::internal(code, message),
            },
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
            details,
        } = err;
        // Validation and not-found keep their specific code in `error` and
        // their message as written, with any details, as Java's envelope
        // does (shared/httperror/HttpError.java:47-49).
        match kind {
            ErrorKind::Validation => PlatformError::Coded {
                status: axum::http::StatusCode::BAD_REQUEST,
                code,
                message,
                details,
            },
            // A conflict that carries details (Java's `VERSION_DIGEST_EXISTS`
            // with `details.version`) keeps them in the body.
            ErrorKind::BusinessRule if !details.is_empty() => PlatformError::Coded {
                status: axum::http::StatusCode::CONFLICT,
                code,
                message,
                details,
            },
            ErrorKind::BusinessRule => PlatformError::BusinessRule { code, message },
            // Unintercepted, a no-op is the historical conflict: same code,
            // same status.
            ErrorKind::Unchanged if !details.is_empty() => PlatformError::Coded {
                status: axum::http::StatusCode::CONFLICT,
                code,
                message,
                details,
            },
            ErrorKind::Unchanged => PlatformError::BusinessRule { code, message },
            ErrorKind::NotFound => PlatformError::Coded {
                status: axum::http::StatusCode::NOT_FOUND,
                code,
                message,
                details,
            },
            ErrorKind::Concurrency => PlatformError::Concurrency { code, message },
            // Java's envelope for an authorization error: its own code
            // (`SCOPE_FORBIDDEN`, `FORBIDDEN`, …) and message.
            ErrorKind::Forbidden => PlatformError::Coded {
                status: axum::http::StatusCode::FORBIDDEN,
                code,
                message,
                details,
            },
            ErrorKind::Internal => PlatformError::Internal {
                message: format!("{}: {}", code, message),
            },
            ErrorKind::Unprocessable => PlatformError::Coded {
                status: axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                code,
                message,
                details,
            },
            ErrorKind::Unavailable => PlatformError::Coded {
                status: axum::http::StatusCode::SERVICE_UNAVAILABLE,
                code,
                message,
                details,
            },
            ErrorKind::PreconditionFailed => PlatformError::Coded {
                status: axum::http::StatusCode::PRECONDITION_FAILED,
                code,
                message,
                details,
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

    /// HTTP status + JSON body of a `PlatformError` response, with `code`
    /// checked to equal `error` and then left out (so the expectations
    /// below read as Java's envelope).
    async fn render(err: PlatformError) -> (u16, serde_json::Value) {
        use axum::response::IntoResponse;
        let resp = err.into_response();
        let status = resp.status().as_u16();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let mut body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let code = body
            .as_object_mut()
            .unwrap()
            .remove("code")
            .expect("every error body carries `code`");
        assert_eq!(code, body["error"], "`code` equals `error`: {body}");
        (status, body)
    }

    /// The raw envelope: `error`, `code`, `message` and `details`, in that
    /// order.
    #[tokio::test]
    async fn every_error_body_carries_code_equal_to_error() {
        use axum::response::IntoResponse;
        let resp = PlatformError::from(UseCaseError::validation_with_details(
            "BAD",
            "bad",
            details! { "field" => "name" },
        ))
        .into_response();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            std::str::from_utf8(&bytes).unwrap(),
            r#"{"error":"BAD","code":"BAD","message":"bad","details":{"field":"name"}}"#
        );
    }

    /// Java's artifact statuses (422, 503) and a conflict's details reach
    /// the body, and survive a round trip through `PlatformError`.
    #[tokio::test]
    async fn unprocessable_unavailable_and_detailed_conflicts_render_as_java() {
        let (status, body) =
            render(UseCaseError::unprocessable("ARTIFACT_EMPTY", "empty").into()).await;
        assert_eq!(status, 422);
        assert_eq!(
            body,
            serde_json::json!({"error": "ARTIFACT_EMPTY", "message": "empty"})
        );
        let (status, body) =
            render(UseCaseError::unavailable("ARTIFACT_STORE_NOT_CONFIGURED", "none").into()).await;
        assert_eq!(
            (status, body["error"].as_str()),
            (503, Some("ARTIFACT_STORE_NOT_CONFIGURED"))
        );
        let mut details = HashMap::new();
        details.insert("version".to_string(), serde_json::json!(3));
        let conflict =
            UseCaseError::business_rule_with_details("VERSION_DIGEST_EXISTS", "dup", details);
        let (status, body) = render(conflict.clone().into()).await;
        assert_eq!(status, 409);
        assert_eq!(
            body,
            serde_json::json!({"error": "VERSION_DIGEST_EXISTS", "message": "dup", "details": {"version": 3}})
        );
        let back = UseCaseError::from(PlatformError::from(conflict));
        assert_eq!(
            (back.kind(), back.details()["version"].as_i64()),
            (ErrorKind::BusinessRule, Some(3))
        );
        for (err, kind) in [
            (
                UseCaseError::unprocessable("A", "a"),
                ErrorKind::Unprocessable,
            ),
            (UseCaseError::unavailable("B", "b"), ErrorKind::Unavailable),
            (
                UseCaseError::precondition_failed("C", "c", HashMap::new()),
                ErrorKind::PreconditionFailed,
            ),
        ] {
            assert_eq!(UseCaseError::from(PlatformError::from(err)).kind(), kind);
        }
        // A conflict with no details keeps the plain body.
        let (_, body) = render(UseCaseError::business_rule("X", "x").into()).await;
        assert!(body.get("details").is_none());
    }

    /// A no-op nobody intercepts renders as the conflict it used to be:
    /// same status, code, message and details.
    #[tokio::test]
    async fn an_unintercepted_no_op_is_the_historical_conflict() {
        let noop =
            UseCaseError::unchanged("VERSION_DIGEST_EXISTS", "dup", details! { "version" => 3 });
        assert!(noop.is_unchanged());
        assert_eq!(noop.http_status_code(), 409);
        let (status, body) = render(noop.into()).await;
        assert_eq!(status, 409);
        assert_eq!(
            body,
            serde_json::json!({"error": "VERSION_DIGEST_EXISTS", "message": "dup", "details": {"version": 3}})
        );
        let (status, body) =
            render(UseCaseError::unchanged("ALIAS_UNCHANGED", "same", HashMap::new()).into()).await;
        assert_eq!(
            (status, body),
            (
                409,
                serde_json::json!({"error": "ALIAS_UNCHANGED", "message": "same"})
            )
        );
        assert!(!UseCaseError::business_rule("X", "x").is_unchanged());
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
                PlatformError::validation("bad"),
                PlatformError::bad_request_code("SOME_CODE", "bad"),
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
            // Java's envelope (HttpError.java): the specific code in
            // `error`, the message as written, details when there are any.
            (
                UseCaseError::validation("NAME_REQUIRED", "Name is required"),
                400,
                json!({"error": "NAME_REQUIRED", "message": "Name is required"}),
            ),
            (
                UseCaseError::validation_with_details("BAD", "bad", details! { "field" => "name" }),
                400,
                json!({"error": "BAD", "message": "bad", "details": {"field": "name"}}),
            ),
            (
                UseCaseError::business_rule("ROLE_IN_USE", "in use"),
                409,
                json!({"error": "ROLE_IN_USE", "message": "in use"}),
            ),
            (
                UseCaseError::not_found("ROLE_NOT_FOUND", "Role 'r1' not found"),
                404,
                json!({"error": "ROLE_NOT_FOUND", "message": "Role 'r1' not found"}),
            ),
            (
                UseCaseError::concurrency("STALE", "stale"),
                409,
                json!({"error": "STALE", "message": "stale"}),
            ),
            (
                UseCaseError::forbidden("SCOPE_FORBIDDEN", "no access to this resource's client"),
                403,
                json!({"error": "SCOPE_FORBIDDEN", "message": "no access to this resource's client"}),
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
