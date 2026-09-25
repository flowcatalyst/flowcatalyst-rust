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

    /// The session endpoints' own envelope `{code, message}` (Go
    /// auth/login/endpoint.go `writeUnauthorized` / `writeTooManyRequests`):
    /// a 401 `UNAUTHENTICATED` carries `WWW-Authenticate: Cookie
    /// realm="fc_session"`, a 429 `TOO_MANY_REQUESTS` carries `Retry-After`.
    #[error("{message}")]
    SessionEndpoint {
        status: StatusCode,
        code: String,
        message: String,
        retry_after_secs: Option<u32>,
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

    /// Go's `writeUnauthorized` on the session endpoints: 401
    /// `{"code": "UNAUTHENTICATED", "message"}` with `WWW-Authenticate:
    /// Cookie realm="fc_session"`.
    pub fn session_unauthorized(message: impl Into<String>) -> Self {
        Self::SessionEndpoint {
            status: StatusCode::UNAUTHORIZED,
            code: "UNAUTHENTICATED".to_string(),
            message: message.into(),
            retry_after_secs: None,
        }
    }

    /// Go's login backoff rejection (`writeTooManyRequests`): 429
    /// `{"code": "TOO_MANY_REQUESTS", "message"}` with `Retry-After`.
    pub fn login_backoff(retry_after_secs: u32) -> Self {
        Self::SessionEndpoint {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "TOO_MANY_REQUESTS".to_string(),
            message: "too many failed login attempts; try again later".to_string(),
            retry_after_secs: Some(retry_after_secs.max(1)),
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

/// Error response body: Go's envelope `{error, message, details?}`
/// (shared/httperror/httperror.go:22-26, shared/httpcompat/httpcompat.go:
/// ErrorModel). `error` carries the code. Go's huma also emits a `$schema`
/// member; Rust does not (owner decision #30).
#[derive(Debug, Clone, serde::Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
    /// Structured details, only when the error has some (Go's
    /// `details,omitempty`).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub details: Option<std::collections::HashMap<String, serde_json::Value>>,
}

/// The function-management contract's rendering of an error (owner
/// decision #5: the function routes follow Java, with a `code` member
/// alongside `error`, UPPER_SNAKE codes and the statuses they had).
/// Every platform error response carries it as an extension;
/// [`keep_function_contract`] re-renders the function routes' errors from
/// it, so Go's envelope on the platform routes never reaches them.
#[derive(Debug, Clone)]
pub struct FunctionContractError {
    pub status: StatusCode,
    pub code: String,
    pub message: String,
    pub details: Option<std::collections::HashMap<String, serde_json::Value>>,
    pub retry_after_secs: Option<u32>,
}

/// Body of a [`FunctionContractError`]: `{error, code, message, details?}`.
#[derive(Debug, serde::Serialize)]
struct FunctionContractBody<'a> {
    error: &'a str,
    code: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<&'a std::collections::HashMap<String, serde_json::Value>>,
}

impl FunctionContractError {
    pub fn into_response(self) -> Response {
        let body = FunctionContractBody {
            error: &self.code,
            code: &self.code,
            message: &self.message,
            details: self.details.as_ref(),
        };
        let mut response = (self.status, Json(body)).into_response();
        if let Some(secs) = self.retry_after_secs {
            if let Ok(v) = axum::http::HeaderValue::from_str(&secs.to_string()) {
                response
                    .headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, v);
            }
        }
        response
    }
}

/// Response mapper for the function-management routes: an error response
/// that carries a [`FunctionContractError`] is re-rendered in that
/// contract (owner decision #5). Everything else passes through.
pub async fn keep_function_contract(response: Response) -> Response {
    match response.extensions().get::<FunctionContractError>() {
        Some(contract) => contract.clone().into_response(),
        None => response,
    }
}

/// A details key marking a code Go spells exactly as given (see
/// [`crate::usecase::UseCaseError::not_found_verbatim`]); stripped before the
/// body is rendered.
pub const VERBATIM_CODE: &str = "$verbatimCode";

/// Go's name for a resource in a not-found code, from the name Rust used:
/// `APPLICATION` / `Application` → `Application`, `OAUTH_CLIENT` →
/// `OAuthClient`, `ClientAuthConfig` → `AuthConfig`. Go builds every
/// not-found code as `<Resource>_NOT_FOUND` with the resource spelt as
/// written (`httperror.NotFound`, shared/httperror/httperror.go:91-96).
/// `None` for a name Go never uses (a function-runner resource, say), which
/// keeps its code as is.
pub fn go_resource_name(name: &str) -> Option<&'static str> {
    const NAMES: &[&str] = &[
        "AnchorDomain",
        "Application",
        "AuditLog",
        "AuthConfig",
        "Client",
        "ClientConfig",
        "Config",
        "Connection",
        "CorsOrigin",
        "Credential",
        "DispatchJob",
        "DispatchPool",
        "Doc",
        "EmailDomainMapping",
        "Event",
        "EventType",
        "Grant",
        "IdentityProvider",
        "IdpRoleMapping",
        "OAuthClient",
        "OpenApiSpec",
        "Permission",
        "PlatformConfigAccess",
        "PortalApp",
        "PortalIdentity",
        "Principal",
        "Process",
        "ResetApprovalRequest",
        "Role",
        "ScheduledJob",
        "ScheduledJobInstance",
        "ServiceAccount",
        "ServiceAccountPrincipal",
        "SpecVersion",
        "Subscription",
        "User",
        "WebauthnCredential",
    ];
    // Rust's own names for resources Go names differently.
    let aliased = match name {
        "ClientAuthConfig" | "CLIENT_AUTH_CONFIG" => "AuthConfig",
        "ApplicationClientConfig" | "APPLICATION_CLIENT_CONFIG" => "ClientConfig",
        "CorsAllowedOrigin" | "CORS_ALLOWED_ORIGIN" | "CORS origin" => "CorsOrigin",
        "OAuth client" | "OAUTH_CLIENT" => "OAuthClient",
        "Mapping" | "MAPPING" | "IDP_ROLE_MAPPING" => "IdpRoleMapping",
        "Passkey" | "PASSKEY" => "WebauthnCredential",
        other => other,
    };
    let folded: String = aliased
        .chars()
        .filter(|c| *c != '_' && *c != ' ' && *c != '-')
        .collect();
    NAMES
        .iter()
        .find(|n| n.eq_ignore_ascii_case(&folded))
        .copied()
}

/// Go's not-found `(code, message)` for a resource and id:
/// `<Resource>_NOT_FOUND`, `<Resource> not found: <id>`.
pub fn go_not_found(resource: &str, id: &str) -> (String, String) {
    (
        format!("{resource}_NOT_FOUND"),
        format!("{resource} not found: {id}"),
    )
}

/// A resource name without a qualifier: `OpenApiSpec(current)` →
/// `OpenApiSpec`.
fn bare_resource(name: &str) -> &str {
    name.split('(').next().unwrap_or(name).trim()
}

/// An id without its key: `application_id=app_1` → `app_1`.
fn bare_id(id: &str) -> &str {
    match id.split_once('=') {
        Some((key, value)) if !key.contains(' ') => value,
        _ => id,
    }
}

/// The id a Rust not-found message names: the first `'…'` segment, or what
/// follows `not found: `.
fn id_in_message(message: &str) -> Option<&str> {
    if let Some(start) = message.find('\'') {
        let rest = &message[start + 1..];
        if let Some(end) = rest.find('\'') {
            return Some(&rest[..end]);
        }
    }
    message
        .split_once("not found: ")
        .map(|(_, id)| id.trim())
        .filter(|id| !id.is_empty())
}

/// A not-found error with a code, in Go's form when Go names the resource.
/// Rust's legacy `NOT_FOUND` + `Entity not found: X with id Y` (a
/// [`PlatformError::NotFound`] that went through a use case) is read back
/// into its parts.
fn go_coded_not_found(code: &str, message: &str) -> Option<(String, String)> {
    if code == "NOT_FOUND" {
        if let Some(rest) = message.strip_prefix("Entity not found: ") {
            let (entity, id) = rest.split_once(" with id ")?;
            let resource = go_resource_name(bare_resource(entity))?;
            return Some(go_not_found(resource, bare_id(id)));
        }
        // A generic code with the resource in the text: "Identity provider
        // with ID 'x' not found", "CORS origin 'x' not found".
        let words = message
            .split(" with ")
            .next()
            .and_then(|head| head.split(" '").next())
            .and_then(|head| head.split(" not found").next())?;
        let resource = go_resource_name(words)?;
        let id = id_in_message(message)?;
        return Some(go_not_found(resource, id));
    }
    let raw = code.strip_suffix("_NOT_FOUND")?;
    let resource = go_resource_name(raw)?;
    let id = id_in_message(message);
    Some(match id {
        Some(id) => go_not_found(resource, id),
        None => (format!("{resource}_NOT_FOUND"), message.to_string()),
    })
}

/// Go's `(code, message)` for a platform error: the envelope Go's
/// `httperror.Write` / huma `ErrorModel` render. `legacy` is the
/// function-contract rendering, from which most codes carry over as is.
fn go_code_and_message(
    err: &PlatformError,
    legacy_code: &str,
    legacy_message: &str,
) -> (String, String) {
    // A resource Go never names keeps the rendering it had.
    let named = |resource: &str, id: &str| match go_resource_name(bare_resource(resource)) {
        Some(r) => go_not_found(r, bare_id(id)),
        None => (legacy_code.to_string(), legacy_message.to_string()),
    };
    match err {
        PlatformError::NotFound { entity_type, id } => named(entity_type, id),
        PlatformError::EventTypeNotFound { code } => named("EventType", code),
        PlatformError::SubscriptionNotFound { code } => named("Subscription", code),
        PlatformError::ClientNotFound { id } => named("Client", id),
        PlatformError::PrincipalNotFound { id } => named("Principal", id),
        PlatformError::ServiceAccountNotFound { id } => named("ServiceAccount", id),
        PlatformError::Duplicate {
            entity_type,
            field,
            value,
        } => {
            if field == "unique" {
                ("CONFLICT".to_string(), value.clone())
            } else {
                (
                    format!("{}_EXISTS", field.to_ascii_uppercase()),
                    format!("{entity_type} with {field} '{value}' already exists"),
                )
            }
        }
        PlatformError::Validation { message } => ("VALIDATION".to_string(), message.clone()),
        PlatformError::Forbidden { message } => ("FORBIDDEN".to_string(), message.clone()),
        PlatformError::Unauthorized { message } => ("UNAUTHORIZED".to_string(), message.clone()),
        PlatformError::Coded {
            status,
            code,
            message,
            ..
        } => {
            if *status == StatusCode::NOT_FOUND {
                if let Some(pair) = go_coded_not_found(code, message) {
                    return pair;
                }
            }
            if code == "VALIDATION_ERROR" {
                let message = message
                    .strip_prefix("Validation error: ")
                    .unwrap_or(message);
                return ("VALIDATION".to_string(), message.to_string());
            }
            if *status == StatusCode::INTERNAL_SERVER_ERROR {
                return (code.clone(), "Internal server error".to_string());
            }
            (code.clone(), message.clone())
        }
        PlatformError::Sqlx(_)
        | PlatformError::Json(_)
        | PlatformError::Configuration { .. }
        | PlatformError::Internal { .. } => {
            ("INTERNAL".to_string(), "Internal server error".to_string())
        }
        _ => (legacy_code.to_string(), legacy_message.to_string()),
    }
}

impl IntoResponse for PlatformError {
    fn into_response(self) -> Response {
        if let PlatformError::SessionEndpoint {
            status,
            code,
            message,
            retry_after_secs,
        } = self
        {
            let mut response = (
                status,
                Json(serde_json::json!({"code": code, "message": message})),
            )
                .into_response();
            let headers = response.headers_mut();
            if status == StatusCode::UNAUTHORIZED {
                headers.insert(
                    axum::http::header::WWW_AUTHENTICATE,
                    axum::http::HeaderValue::from_static(r#"Cookie realm="fc_session""#),
                );
            }
            if let Some(secs) = retry_after_secs {
                if let Ok(v) = axum::http::HeaderValue::from_str(&secs.to_string()) {
                    headers.insert(axum::http::header::RETRY_AFTER, v);
                }
            }
            return response;
        }
        let (status, error_code) = match &self {
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
        };

        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "Internal server error");
        }

        // A 500 carries its code and a fixed message; the cause (SQL error
        // text, internal detail) is only logged above, never sent, as in Go
        // (shared/httperror: an internal error's cause is logged, not
        // serialised).
        let legacy_message = if status == StatusCode::INTERNAL_SERVER_ERROR {
            "Internal server error".to_string()
        } else {
            self.to_string()
        };
        let verbatim = matches!(
            &self,
            PlatformError::Coded { details, .. } if details.contains_key(VERBATIM_CODE)
        );
        let (code, message) = if verbatim {
            (error_code.clone(), legacy_message.clone())
        } else {
            go_code_and_message(&self, &error_code, &legacy_message)
        };
        // 429 carries Retry-After per RFC 6585 / 7231.
        let retry_after_secs = match &self {
            PlatformError::TooManyRequests {
                retry_after_secs, ..
            } => Some(*retry_after_secs),
            _ => None,
        };
        let details = match self {
            PlatformError::Coded { mut details, .. } => {
                details.remove(VERBATIM_CODE);
                (!details.is_empty()).then_some(details)
            }
            _ => None,
        };
        let contract = FunctionContractError {
            status,
            code: error_code,
            message: legacy_message,
            details: details.clone(),
            retry_after_secs,
        };
        let body = ErrorResponse {
            error: code,
            message,
            details,
        };

        let mut response = (status, Json(body)).into_response();
        if let Some(secs) = retry_after_secs {
            if let Ok(v) = axum::http::HeaderValue::from_str(&secs.to_string()) {
                response
                    .headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, v);
            }
        }
        response.extensions_mut().insert(contract);
        response
    }
}

#[cfg(test)]
mod go_envelope_tests {
    use super::*;
    use http_body_util::BodyExt;

    async fn body(err: PlatformError) -> (StatusCode, serde_json::Value) {
        let response = err.into_response();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn not_found_is_gos_resource_code_and_message() {
        let (status, v) = body(PlatformError::not_found("Application", "app_1")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            v,
            serde_json::json!({"error": "Application_NOT_FOUND", "message": "Application not found: app_1"})
        );
    }

    #[tokio::test]
    async fn use_case_not_found_codes_take_gos_spelling() {
        let err: PlatformError = crate::usecase::UseCaseError::not_found(
            "OAUTH_CLIENT_NOT_FOUND",
            "OAuth client 'oc_1' not found",
        )
        .into();
        let (_, v) = body(err).await;
        assert_eq!(v["error"], "OAuthClient_NOT_FOUND");
        assert_eq!(v["message"], "OAuthClient not found: oc_1");
        assert!(v.get("code").is_none());
    }

    #[tokio::test]
    async fn unknown_resources_keep_their_code() {
        let err = PlatformError::not_found_code("FunctionVersion", "v1");
        let (_, v) = body(err).await;
        assert_eq!(v["error"], "FUNCTION_VERSION_NOT_FOUND");
    }

    #[tokio::test]
    async fn the_function_contract_keeps_code_and_legacy_codes() {
        let response =
            keep_function_contract(PlatformError::not_found("Function", "f").into_response()).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"], "NOT_FOUND");
        assert_eq!(v["code"], "NOT_FOUND");
    }

    #[tokio::test]
    async fn validation_is_gos_validation_code() {
        let (status, v) = body(PlatformError::validation("name is required")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            v,
            serde_json::json!({"error": "VALIDATION", "message": "name is required"})
        );
    }
}
