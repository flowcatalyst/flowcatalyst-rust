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

    #[error("Validation error: {message}")]
    Validation { message: String },

    #[error("Authorization error: {message}")]
    Unauthorized { message: String },

    #[error("Forbidden: {message}")]
    Forbidden { message: String },

    #[error("Database error: {0}")]
    Database(#[from] sea_orm::DbErr),

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
}

pub type Result<T> = std::result::Result<T, PlatformError>;

/// Error response body
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
}

impl IntoResponse for PlatformError {
    fn into_response(self) -> Response {
        let (status, error_type) = match &self {
            PlatformError::NotFound { .. } => (StatusCode::NOT_FOUND, "NOT_FOUND"),
            PlatformError::Duplicate { .. } => (StatusCode::CONFLICT, "DUPLICATE"),
            PlatformError::Validation { .. } => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR"),
            PlatformError::Unauthorized { .. } => (StatusCode::UNAUTHORIZED, "UNAUTHORIZED"),
            PlatformError::Forbidden { .. } => (StatusCode::FORBIDDEN, "FORBIDDEN"),
            PlatformError::InvalidCredentials => (StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS"),
            PlatformError::TokenExpired => (StatusCode::UNAUTHORIZED, "TOKEN_EXPIRED"),
            PlatformError::InvalidToken { .. } => (StatusCode::UNAUTHORIZED, "INVALID_TOKEN"),
            PlatformError::SchemaValidation { .. } => (StatusCode::BAD_REQUEST, "SCHEMA_ERROR"),
            PlatformError::EventTypeNotFound { .. } => (StatusCode::NOT_FOUND, "EVENT_TYPE_NOT_FOUND"),
            PlatformError::SubscriptionNotFound { .. } => (StatusCode::NOT_FOUND, "SUBSCRIPTION_NOT_FOUND"),
            PlatformError::ClientNotFound { .. } => (StatusCode::NOT_FOUND, "CLIENT_NOT_FOUND"),
            PlatformError::PrincipalNotFound { .. } => (StatusCode::NOT_FOUND, "PRINCIPAL_NOT_FOUND"),
            PlatformError::ServiceAccountNotFound { .. } => (StatusCode::NOT_FOUND, "SERVICE_ACCOUNT_NOT_FOUND"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        };

        let body = ErrorResponse {
            error: error_type.to_string(),
            message: self.to_string(),
        };

        (status, Json(body)).into_response()
    }
}

impl From<UseCaseError> for PlatformError {
    fn from(err: UseCaseError) -> Self {
        match err {
            UseCaseError::ValidationError { message, .. } => {
                PlatformError::Validation { message }
            }
            UseCaseError::BusinessRuleViolation { message, .. } => {
                PlatformError::Duplicate {
                    entity_type: "Entity".to_string(),
                    field: "constraint".to_string(),
                    value: message,
                }
            }
            UseCaseError::NotFoundError { message, .. } => {
                PlatformError::NotFound {
                    entity_type: "Entity".to_string(),
                    id: message,
                }
            }
            UseCaseError::ConcurrencyError { message, .. } => {
                PlatformError::Internal { message }
            }
            UseCaseError::CommitError { message, .. } => {
                PlatformError::Internal { message }
            }
        }
    }
}
