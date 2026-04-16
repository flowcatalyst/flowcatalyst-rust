//! Platform Error Types

use thiserror::Error;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response, Json},
};
use utoipa::ToSchema;

use crate::usecase::UseCaseError;

#[derive(Error, Debug)]
pub enum PlatformError {
    #[error("Entity not found: {entity_type} with id {id}")]
    NotFound { entity_type: String, id: String },

    #[error("Duplicate entity: {entity_type} with {field}={value}")]
    Duplicate { entity_type: String, field: String, value: String },

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

    #[error("Invalid TSID: {0}")]
    InvalidTsid(String),

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

    #[error("Schema validation failed: {message}")]
    SchemaValidation { message: String },

    #[error("Dispatch error: {message}")]
    Dispatch { message: String },

    #[error("Internal error: {message}")]
    Internal { message: String },
}

impl PlatformError {
    pub fn not_found(entity_type: impl Into<String>, id: impl Into<String>) -> Self {
        Self::NotFound {
            entity_type: entity_type.into(),
            id: id.into(),
        }
    }

    pub fn duplicate(entity_type: impl Into<String>, field: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Duplicate {
            entity_type: entity_type.into(),
            field: field.into(),
            value: value.into(),
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation { message: message.into() }
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized { message: message.into() }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden { message: message.into() }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal { message: message.into() }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::Validation { message: message.into() }
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

/// Error response body
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
}

impl IntoResponse for PlatformError {
    fn into_response(self) -> Response {
        let (status, error_code) = match &self {
            PlatformError::NotFound { .. } => (StatusCode::NOT_FOUND, "NOT_FOUND".to_string()),
            PlatformError::Duplicate { .. } => (StatusCode::CONFLICT, "DUPLICATE".to_string()),
            PlatformError::BusinessRule { code, .. } => (StatusCode::CONFLICT, code.clone()),
            PlatformError::Concurrency { code, .. } => (StatusCode::CONFLICT, code.clone()),
            PlatformError::Validation { .. } => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR".to_string()),
            PlatformError::Unauthorized { .. } => (StatusCode::UNAUTHORIZED, "UNAUTHORIZED".to_string()),
            PlatformError::Forbidden { .. } => (StatusCode::FORBIDDEN, "FORBIDDEN".to_string()),
            PlatformError::InvalidCredentials => (StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS".to_string()),
            PlatformError::TokenExpired => (StatusCode::UNAUTHORIZED, "TOKEN_EXPIRED".to_string()),
            PlatformError::InvalidToken { .. } => (StatusCode::UNAUTHORIZED, "INVALID_TOKEN".to_string()),
            PlatformError::SchemaValidation { .. } => (StatusCode::BAD_REQUEST, "SCHEMA_ERROR".to_string()),
            PlatformError::EventTypeNotFound { .. } => (StatusCode::NOT_FOUND, "EVENT_TYPE_NOT_FOUND".to_string()),
            PlatformError::SubscriptionNotFound { .. } => (StatusCode::NOT_FOUND, "SUBSCRIPTION_NOT_FOUND".to_string()),
            PlatformError::ClientNotFound { .. } => (StatusCode::NOT_FOUND, "CLIENT_NOT_FOUND".to_string()),
            PlatformError::PrincipalNotFound { .. } => (StatusCode::NOT_FOUND, "PRINCIPAL_NOT_FOUND".to_string()),
            PlatformError::ServiceAccountNotFound { .. } => (StatusCode::NOT_FOUND, "SERVICE_ACCOUNT_NOT_FOUND".to_string()),
            PlatformError::Sqlx(_) => (StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR".to_string()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR".to_string()),
        };

        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "Internal server error");
        }

        let body = ErrorResponse {
            error: error_code,
            message: self.to_string(),
        };

        (status, Json(body)).into_response()
    }
}

impl From<UseCaseError> for PlatformError {
    fn from(err: UseCaseError) -> Self {
        match err {
            UseCaseError::ValidationError { code, message, .. } => {
                PlatformError::Validation {
                    message: format!("{}: {}", code, message),
                }
            }
            UseCaseError::BusinessRuleViolation { code, message, .. } => {
                PlatformError::BusinessRule { code, message }
            }
            UseCaseError::NotFoundError { code, message, .. } => {
                PlatformError::NotFound {
                    entity_type: code,
                    id: message,
                }
            }
            UseCaseError::ConcurrencyError { code, message, .. } => {
                PlatformError::Concurrency { code, message }
            }
            UseCaseError::CommitError { code, message, .. } => {
                PlatformError::Internal {
                    message: format!("{}: {}", code, message),
                }
            }
        }
    }
}
