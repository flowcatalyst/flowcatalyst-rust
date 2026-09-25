//! Platform Error Types

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use thiserror::Error;
use utoipa::ToSchema;

#[derive(Error, Debug)]
pub enum PlatformError {
    #[error("Entity not found: {entity_type} with id {id}")]
    NotFound { entity_type: String, id: String },

    #[error("Duplicate entity: {entity_type} with {field}={value}")]
    Duplicate {
        entity_type: String,
        field: String,
        value: String,
    },

    #[error("{message}")]
    BusinessRule { code: String, message: String },

    #[error("{message}")]
    Concurrency { code: String, message: String },

    #[error("Validation error: {message}")]
    Validation { message: String },

    #[error("Authorization error: {message}")]
    Unauthorized { message: String },

    #[error("Forbidden: {message}")]
    Forbidden { message: String },

    #[error("SQL error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Configuration error: {message}")]
    Configuration { message: String },

    #[error("Event type not found: {code}")]
    EventTypeNotFound { code: String },

    #[error("Subscription not found: {code}")]
    SubscriptionNotFound { code: String },

    #[error("Client not found: {id}")]
    ClientNotFound { id: String },

    #[error("Principal not found: {id}")]
    PrincipalNotFound { id: String },

    #[error("Service account not found: {id}")]
    ServiceAccountNotFound { id: String },

    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Token expired")]
    TokenExpired,

    #[error("Invalid token: {message}")]
    InvalidToken { message: String },

    #[error("Internal error: {message}")]
    Internal { message: String },

    #[error("{message}")]
    TooManyRequests {
        retry_after_secs: u32,
        message: String,
    },

    /// An error that carries its own code, rendered as Java's envelope
    /// `{"error": code, "message": message, "details"?: {...}}`
    /// (flowcatalyst-javalin shared/httperror/HttpError.java): `error` is
    /// the specific code, `message` is sent as written, and `details` only
    /// when there are any. Use-case validation and not-found errors arrive
    /// here.
    #[error("{message}")]
    Coded {
        status: StatusCode,
        code: String,
        message: String,
        details: std::collections::HashMap<String, serde_json::Value>,
    },
}

impl PlatformError {
    pub fn not_found(entity_type: impl Into<String>, id: impl Into<String>) -> Self {
        Self::NotFound {
            entity_type: entity_type.into(),
            id: id.into(),
        }
    }

    pub fn duplicate(
        entity_type: impl Into<String>,
        field: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self::Duplicate {
            entity_type: entity_type.into(),
            field: field.into(),
            value: value.into(),
        }
    }

    /// A 400 with a specific code (Java's `HttpError.badRequest`).
    pub fn bad_request_code(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Coded {
            status: StatusCode::BAD_REQUEST,
            code: code.into(),
            message: message.into(),
            details: Default::default(),
        }
    }

    /// Go's `httperror.NotFound(resource, id)` (shared/httperror/
    /// httperror.go:91-96): 404 `<RESOURCE>_NOT_FOUND` and `<Resource> not
    /// found: <id>`. The code is UPPER_SNAKE (the free function
    /// `not_found_code`), as every other code is (owner decision 5); Go and
    /// Java append `_NOT_FOUND` to the resource name as given.
    pub fn not_found_code(resource: &str, id: impl std::fmt::Display) -> Self {
        Self::Coded {
            status: StatusCode::NOT_FOUND,
            code: not_found_code(resource),
            message: format!("{resource} not found: {id}"),
            details: Default::default(),
        }
    }

    /// A 403 with a specific code (Go's `usecase.Authorization`, rendered
    /// by shared/httperror/httperror.go:55-80 as `{"error": code, "message"}`).
    pub fn forbidden_code(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Coded {
            status: StatusCode::FORBIDDEN,
            code: code.into(),
            message: message.into(),
            details: Default::default(),
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation {
            message: message.into(),
        }
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized {
            message: message.into(),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden {
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::Validation {
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Duplicate {
            entity_type: "Entity".to_string(),
            field: "unique".to_string(),
            value: message.into(),
        }
    }

    pub fn business_rule(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::BusinessRule {
            code: code.into(),
            message: message.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, PlatformError>;

/// The not-found code for a resource type name: `<RESOURCE>_NOT_FOUND` in
/// UPPER_SNAKE (`FunctionVersion` → `FUNCTION_VERSION_NOT_FOUND`), the one
/// place such a code is built from a name.
pub fn not_found_code(resource: &str) -> String {
    let mut code = String::with_capacity(resource.len() + 12);
    let mut previous: Option<char> = None;
    for c in resource.chars() {
        let boundary = c.is_ascii_uppercase()
            && previous.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit());
        if boundary {
            code.push('_');
        }
        if c == ' ' || c == '-' {
            code.push('_');
        } else {
            code.push(c.to_ascii_uppercase());
        }
        previous = Some(c);
    }
    code.push_str("_NOT_FOUND");
    code
}

#[cfg(test)]
mod not_found_code_tests {
    use super::not_found_code;

    #[test]
    fn resource_names_become_upper_snake() {
        for (resource, code) in [
            ("Function", "FUNCTION_NOT_FOUND"),
            ("FunctionVersion", "FUNCTION_VERSION_NOT_FOUND"),
            ("FunctionDomain", "FUNCTION_DOMAIN_NOT_FOUND"),
            ("FunctionSecret", "FUNCTION_SECRET_NOT_FOUND"),
            ("Alias", "ALIAS_NOT_FOUND"),
            ("Config", "CONFIG_NOT_FOUND"),
            ("OAuthClient", "OAUTH_CLIENT_NOT_FOUND"),
            ("EVENT_TYPE", "EVENT_TYPE_NOT_FOUND"),
        ] {
            assert_eq!(not_found_code(resource), code, "{resource}");
        }
    }
}

/// Extension trait for `Option<T>` to convert `None` into `PlatformError::not_found`.
///
/// Replaces the verbose `.ok_or_else(|| PlatformError::not_found("Entity", &id))?` pattern.
///
/// # Example
/// ```ignore
/// use crate::shared::error::NotFoundExt;
///
/// let client = state.repo.find_by_id(&id).await?
///     .or_not_found("Client", &id)?;
/// ```
pub trait NotFoundExt<T> {
    fn or_not_found(self, entity_type: &str, id: &str) -> Result<T>;
}

impl<T> NotFoundExt<T> for Option<T> {
    fn or_not_found(self, entity_type: &str, id: &str) -> Result<T> {
        self.ok_or_else(|| PlatformError::not_found(entity_type, id))
    }
}

/// Error response body: `{error, code, message, details?}`. `code` always
/// equals `error` (owner decision 5, additive): `error` is the envelope Go
/// and Java send, `code` the name clients reach for first.
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
    /// The same machine-readable code as `error`.
    pub code: String,
    pub message: String,
    /// Structured details, only when the error has some (Java's
    /// `@JsonInclude(NON_EMPTY)` on `HttpError.details`).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub details: Option<std::collections::HashMap<String, serde_json::Value>>,
}

impl PlatformError {
    /// The HTTP status and machine-readable code this error renders with.
    fn status_and_code(&self) -> (StatusCode, String) {
        match self {
            PlatformError::NotFound { .. } => (StatusCode::NOT_FOUND, "NOT_FOUND".to_string()),
            PlatformError::Duplicate { .. } => (StatusCode::CONFLICT, "DUPLICATE".to_string()),
            PlatformError::BusinessRule { code, .. } => (StatusCode::CONFLICT, code.clone()),
            PlatformError::Concurrency { code, .. } => (StatusCode::CONFLICT, code.clone()),
            PlatformError::Validation { .. } => {
                (StatusCode::BAD_REQUEST, "VALIDATION_ERROR".to_string())
            }
            PlatformError::Unauthorized { .. } => {
                (StatusCode::UNAUTHORIZED, "UNAUTHORIZED".to_string())
            }
            PlatformError::Forbidden { .. } => (StatusCode::FORBIDDEN, "FORBIDDEN".to_string()),
            PlatformError::InvalidCredentials => {
                (StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS".to_string())
            }
            PlatformError::TokenExpired => (StatusCode::UNAUTHORIZED, "TOKEN_EXPIRED".to_string()),
            PlatformError::InvalidToken { .. } => {
                (StatusCode::UNAUTHORIZED, "INVALID_TOKEN".to_string())
            }
            PlatformError::EventTypeNotFound { .. } => {
                (StatusCode::NOT_FOUND, "EVENT_TYPE_NOT_FOUND".to_string())
            }
            PlatformError::SubscriptionNotFound { .. } => {
                (StatusCode::NOT_FOUND, "SUBSCRIPTION_NOT_FOUND".to_string())
            }
            PlatformError::ClientNotFound { .. } => {
                (StatusCode::NOT_FOUND, "CLIENT_NOT_FOUND".to_string())
            }
            PlatformError::PrincipalNotFound { .. } => {
                (StatusCode::NOT_FOUND, "PRINCIPAL_NOT_FOUND".to_string())
            }
            PlatformError::ServiceAccountNotFound { .. } => (
                StatusCode::NOT_FOUND,
                "SERVICE_ACCOUNT_NOT_FOUND".to_string(),
            ),
            PlatformError::Sqlx(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR".to_string(),
            ),
            PlatformError::TooManyRequests { .. } => (
                StatusCode::TOO_MANY_REQUESTS,
                "TOO_MANY_REQUESTS".to_string(),
            ),
            PlatformError::Coded { status, code, .. } => (*status, code.clone()),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR".to_string(),
            ),
        }
    }

    /// The HTTP status this error renders with, for callers that map it
    /// onto another response type (the server-rendered `fc-web` UI).
    pub fn status_code(&self) -> StatusCode {
        self.status_and_code().0
    }
}

impl IntoResponse for PlatformError {
    fn into_response(self) -> Response {
        let (status, error_code) = self.status_and_code();

        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "Internal server error");
        }

        // 429 carries Retry-After per RFC 6585 / 7231.
        if let PlatformError::TooManyRequests {
            retry_after_secs, ..
        } = &self
        {
            let body = ErrorResponse {
                code: error_code.clone(),
                error: error_code,
                message: self.to_string(),
                details: None,
            };
            return (
                status,
                [(
                    axum::http::header::RETRY_AFTER,
                    retry_after_secs.to_string(),
                )],
                Json(body),
            )
                .into_response();
        }

        // A 500 carries its code and a fixed message; the cause (SQL error
        // text, internal detail) is only logged above, never sent, as in Go
        // (shared/httperror: an internal error's cause is logged, not
        // serialised).
        let message = if status == StatusCode::INTERNAL_SERVER_ERROR {
            "Internal server error".to_string()
        } else {
            self.to_string()
        };
        let details = match self {
            PlatformError::Coded { details, .. } if !details.is_empty() => Some(details),
            _ => None,
        };
        let body = ErrorResponse {
            code: error_code.clone(),
            error: error_code,
            message,
            details,
        };

        (status, Json(body)).into_response()
    }
}
