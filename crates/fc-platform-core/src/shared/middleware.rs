//! API Middleware
//!
//! Authentication and authorization middleware for Axum.
//! Supports both Bearer token (Authorization header) and session cookie authentication.

use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, header::COOKIE, request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

/// Client IP address extracted from proxy headers.
///
/// **Configured via `FC_TRUSTED_PROXY_HOPS`** (default `1`). Reads
/// `X-Forwarded-For` and walks the chain from the **right** by that many
/// hops — the rightmost entries are added by trusted proxies and the
/// leftmost ones may be attacker-supplied.
///
/// AWS ALB **appends** the real client IP to whatever the client sent, so
/// reading the leftmost value is attacker-controllable. Reading the right
/// (with hop count = number of trusted proxies in front of us) gives the
/// real client. With ALB-only set hops to `1`. With CloudFront → ALB set to
/// `2`. With no proxy at all set to `0` (and the value is whatever the
/// client supplied — only safe in dev).
///
/// Falls back to `X-Real-IP` when no `X-Forwarded-For` is present, and then
/// to the connection's peer address (Go `ratelimit.ClientIP` falls back to
/// `RemoteAddr`), so a login attempt from a directly connected client still
/// records its IP and still counts toward its backoff.
#[derive(Debug, Clone)]
pub struct ClientIp(pub Option<String>);

/// Extract the trusted client IP from `X-Forwarded-For` per the configured
/// trusted-proxy hop count. Public so other code paths (rate-limit
/// middleware) can use the same logic.
pub fn extract_trusted_client_ip(headers: &axum::http::HeaderMap) -> Option<String> {
    let hops = trusted_proxy_hops();
    if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        let chain: Vec<&str> = forwarded
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if !chain.is_empty() {
            // Take the entry `hops` from the right — that's the IP the
            // outermost trusted proxy saw before appending its own.
            // hops=0 means "trust the leftmost as-is" (no proxy in front).
            let idx = chain.len().saturating_sub(hops.max(1));
            return Some(chain[idx].to_string());
        }
    }
    headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn trusted_proxy_hops() -> usize {
    static CACHED: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("FC_TRUSTED_PROXY_HOPS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1)
    })
}

impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(ClientIp(extract_trusted_client_ip(&parts.headers).or_else(
            || {
                parts
                    .extensions
                    .get::<fc_http_listener::PeerAddr>()
                    .map(|peer| match peer.0.ip() {
                        std::net::IpAddr::V6(v6) => v6
                            .to_ipv4_mapped()
                            .map_or_else(|| v6.to_string(), |v4| v4.to_string()),
                        v4 => v4.to_string(),
                    })
            },
        )))
    }
}
use crate::shared::api_common::ApiError;
use crate::shared::authorization_service::AuthContext;
use std::sync::Arc;

/// The platform session cookie's name (Go `SessionCookieName`,
/// shared/middleware/middleware.go:115).
pub const SESSION_COOKIE_NAME: &str = "fc_session";

/// Authenticates a request's credential into an [`AuthContext`]. The
/// platform's `AppState` (fc-platform-iam: the token service and the
/// authorization service) implements it; [`AuthLayer`] puts one on every
/// request for the extractors below.
#[async_trait::async_trait]
pub trait TokenAuthenticator: Send + Sync + 'static {
    /// A bearer access token: the caller's context, or why the token does
    /// not authenticate (its `error_description`).
    async fn bearer_context(&self, token: &str) -> crate::shared::error::Result<AuthContext>;

    /// A session cookie: the signed-in principal's context, `Ok(None)` when
    /// the cookie signs no one in (invalid, expired, or its principal
    /// unknown, inactive or no USER), an error when the lookup failed.
    async fn session_context(
        &self,
        token: &str,
    ) -> crate::shared::error::Result<Option<AuthContext>>;
}

/// The authenticator [`AuthLayer`] puts in a request's extensions.
#[derive(Clone)]
pub struct Authenticator(pub Arc<dyn TokenAuthenticator>);

impl Authenticator {
    pub fn new(authenticator: impl TokenAuthenticator) -> Self {
        Self(Arc::new(authenticator))
    }
}

/// Authenticated user extractor
/// Validates JWT and extracts AuthContext from the request
#[derive(Debug)]
pub struct Authenticated(pub AuthContext);

impl std::ops::Deref for Authenticated {
    type Target = AuthContext;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Error response for authentication failures, as Go's `Authenticator`
/// and permission helpers answer them (shared/middleware/middleware.go,
/// shared/auth/auth.go):
/// - 401: a bearer token that does not validate (bad signature, expired,
///   an identity-only token): `{"error": "invalid_token",
///   "error_description": …}` with `WWW-Authenticate: Bearer
///   error="invalid_token"` (`writeInvalidTokenError`);
/// - 403: no credential, or a session cookie that no longer signs anyone
///   in: `{"error": "UNAUTHENTICATED", "message": "authentication
///   required"}` (`usecase.Authorization("UNAUTHENTICATED", …)`).
///
/// The function routes keep their contract (owner decision #5): a 401
/// `UNAUTHORIZED` with `code`, carried as a
/// [`FunctionContractError`](crate::shared::error::FunctionContractError).
#[derive(Debug)]
pub struct AuthError {
    pub status: StatusCode,
    pub message: String,
}

impl AuthError {
    /// A bearer token that does not authenticate: 401 `invalid_token`.
    pub fn invalid_token(description: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: description.into(),
        }
    }

    /// No credential (or a stale session cookie): 403 `UNAUTHENTICATED`.
    pub fn unauthenticated(legacy_message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: legacy_message.into(),
        }
    }
}

/// Go's `error_description` for a bearer that fails validation.
fn invalid_token_description(e: &crate::PlatformError) -> String {
    match e {
        crate::PlatformError::TokenExpired => {
            "token has invalid claims: token is expired".to_string()
        }
        crate::PlatformError::InvalidToken { message } => message.clone(),
        other @ (crate::PlatformError::NotFound { .. }
        | crate::PlatformError::Duplicate { .. }
        | crate::PlatformError::BusinessRule { .. }
        | crate::PlatformError::Concurrency { .. }
        | crate::PlatformError::Validation { .. }
        | crate::PlatformError::Unauthorized { .. }
        | crate::PlatformError::Forbidden { .. }
        | crate::PlatformError::Sqlx(_)
        | crate::PlatformError::Json(_)
        | crate::PlatformError::Configuration { .. }
        | crate::PlatformError::EventTypeNotFound { .. }
        | crate::PlatformError::SubscriptionNotFound { .. }
        | crate::PlatformError::ClientNotFound { .. }
        | crate::PlatformError::PrincipalNotFound { .. }
        | crate::PlatformError::ServiceAccountNotFound { .. }
        | crate::PlatformError::InvalidCredentials
        | crate::PlatformError::Internal { .. }
        | crate::PlatformError::TooManyRequests { .. }
        | crate::PlatformError::Coded { .. }
        | crate::PlatformError::SessionEndpoint { .. }) => other.to_string(),
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        use crate::shared::error::FunctionContractError;
        let contract = FunctionContractError {
            status: if self.status == StatusCode::FORBIDDEN {
                StatusCode::UNAUTHORIZED
            } else {
                self.status
            },
            code: "UNAUTHORIZED".to_string(),
            message: self.message.clone(),
            details: None,
            retry_after_secs: None,
        };
        let mut response = match self.status {
            StatusCode::UNAUTHORIZED => (
                StatusCode::UNAUTHORIZED,
                [(
                    axum::http::header::WWW_AUTHENTICATE,
                    r#"Bearer error="invalid_token""#,
                )],
                Json(serde_json::json!({
                    "error": "invalid_token",
                    "error_description": self.message,
                })),
            )
                .into_response(),
            StatusCode::FORBIDDEN => (
                StatusCode::FORBIDDEN,
                Json(ApiError::new("UNAUTHENTICATED", "authentication required")),
            )
                .into_response(),
            status => (
                status,
                Json(ApiError::new("INTERNAL", "Internal server error")),
            )
                .into_response(),
        };
        response.extensions_mut().insert(contract);
        response
    }
}

/// The value of the platform session cookie, if the request carries one
/// (exact name match; an empty value counts as none).
pub fn extract_session_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| name.trim() == SESSION_COOKIE_NAME)
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// The credential a request presents (Go `extractToken`,
/// shared/middleware/middleware.go:122-138): an `Authorization` header
/// decides on its own — a Bearer token, or nothing when it names another
/// scheme (the request declared its intent, so the cookie is not
/// consulted); without one, the session cookie.
enum Presented {
    Bearer(String),
    SessionCookie(String),
    Nothing,
}

fn presented_credential(headers: &axum::http::HeaderMap) -> Presented {
    if let Some(header) = headers.get(AUTHORIZATION) {
        const PREFIX: &str = "Bearer ";
        return match header.to_str() {
            Ok(h) if h.len() > PREFIX.len() && h[..PREFIX.len()].eq_ignore_ascii_case(PREFIX) => {
                match h[PREFIX.len()..].trim() {
                    "" => Presented::Nothing,
                    token => Presented::Bearer(token.to_string()),
                }
            }
            _ => Presented::Nothing,
        };
    }
    match extract_session_cookie(headers) {
        Some(token) => Presented::SessionCookie(token),
        None => Presented::Nothing,
    }
}

/// What a request authenticates as.
enum Authentication {
    Context(AuthContext),
    /// No credential, or a session cookie that no longer signs anyone in
    /// (invalid, expired, or its principal unknown, inactive or no USER):
    /// the browser replays a stale cookie on every call, so it reads as
    /// logged out rather than as an error (Go `Authenticator`).
    Anonymous {
        stale_session: bool,
    },
}

/// Authenticate the request (Go `introspect`,
/// shared/middleware/middleware.go:148-224). A bearer is self-contained;
/// a session cookie carries only its subject, whose principal and
/// authority are reloaded from the database on every request, so a
/// deactivation or role change applies at once.
async fn authenticate(
    app_state: &dyn TokenAuthenticator,
    parts: &Parts,
) -> std::result::Result<Authentication, AuthError> {
    // Resolved once already on this request (the profile-only gate).
    if let Some(ResolvedAuthContext(context)) = parts.extensions.get::<ResolvedAuthContext>() {
        return Ok(Authentication::Context(context.clone()));
    }
    authenticate_credential(app_state, &parts.headers).await
}

/// Authenticate the credential the headers present, bearer or session
/// cookie, with no per-request cache.
async fn authenticate_credential(
    app_state: &dyn TokenAuthenticator,
    headers: &axum::http::HeaderMap,
) -> std::result::Result<Authentication, AuthError> {
    match presented_credential(headers) {
        Presented::Nothing => Ok(Authentication::Anonymous {
            stale_session: false,
        }),
        Presented::Bearer(token) => {
            let unauthorized =
                |e: crate::PlatformError| AuthError::invalid_token(invalid_token_description(&e));
            let context = app_state
                .bearer_context(&token)
                .await
                .map_err(unauthorized)?;
            Ok(Authentication::Context(context))
        }
        Presented::SessionCookie(token) => match app_state.session_context(&token).await {
            Ok(Some(context)) => Ok(Authentication::Context(context)),
            Ok(None) => Ok(Authentication::Anonymous {
                stale_session: true,
            }),
            Err(e) => {
                tracing::error!(error = %e, "session principal lookup failed");
                Err(AuthError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "Session lookup failed".to_string(),
                })
            }
        },
    }
}

/// Authenticate a request from its headers alone, for callers outside the
/// axum extractor path (the server-rendered `fc-web` UI). Same rules as
/// [`Authenticated`]: `Ok(None)` is anonymous (no credential, or a session
/// cookie that no longer signs anyone in); a bearer that does not validate,
/// or a failed session lookup, is an error.
pub async fn authenticate_headers<A: TokenAuthenticator>(
    app_state: &A,
    headers: &axum::http::HeaderMap,
) -> std::result::Result<Option<AuthContext>, AuthError> {
    match authenticate_credential(app_state, headers).await? {
        Authentication::Context(context) => Ok(Some(context)),
        Authentication::Anonymous { .. } => Ok(None),
    }
}

/// The caller's context, once a middleware has authenticated the request,
/// so the extractors do not validate the token (or reload a session's
/// principal) a second time.
#[derive(Clone)]
pub struct ResolvedAuthContext(pub AuthContext);

/// The request's authenticated context, or `None` when it presents no
/// credential or one that does not authenticate (the extractors answer
/// those). Caches a success on the request.
pub async fn resolve_context(parts: &mut Parts) -> Option<AuthContext> {
    let app_state = parts.extensions.get::<Authenticator>().cloned()?;
    match authenticate(&*app_state.0, parts).await {
        Ok(Authentication::Context(context)) => {
            parts
                .extensions
                .insert(ResolvedAuthContext(context.clone()));
            Some(context)
        }
        _ => None,
    }
}

impl<S> FromRequestParts<S> for Authenticated
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Get AppState from extensions (set by middleware layer)
        let app_state = parts
            .extensions
            .get::<Authenticator>()
            .cloned()
            .ok_or_else(|| AuthError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "Auth service not configured".to_string(),
            })?;

        match authenticate(&*app_state.0, parts).await? {
            Authentication::Context(context) => Ok(Authenticated(context)),
            Authentication::Anonymous { stale_session } => {
                Err(AuthError::unauthenticated(if stale_session {
                    "Session expired or invalid"
                } else {
                    "Missing authentication token"
                }))
            }
        }
    }
}

/// Optional authentication extractor
/// Tries to validate JWT but allows unauthenticated requests
pub struct OptionalAuth(pub Option<AuthContext>);

impl std::ops::Deref for OptionalAuth {
    type Target = Option<AuthContext>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S> FromRequestParts<S> for OptionalAuth
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Some(app_state) = parts.extensions.get::<Authenticator>().cloned() else {
            return Ok(OptionalAuth(None));
        };
        match authenticate(&*app_state.0, parts).await {
            Ok(Authentication::Context(context)) => Ok(OptionalAuth(Some(context))),
            _ => Ok(OptionalAuth(None)),
        }
    }
}

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
/// Middleware layer that injects the [`Authenticator`] into request extensions
/// This enables the Authenticated extractor to work
use tower::Layer;
use tower::Service;

#[derive(Clone)]
pub struct AuthLayer {
    state: Authenticator,
}

impl AuthLayer {
    pub fn new(state: impl TokenAuthenticator) -> Self {
        Self {
            state: Authenticator::new(state),
        }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthMiddleware {
            inner,
            state: self.state.clone(),
        }
    }
}

#[derive(Clone)]
pub struct AuthMiddleware<S> {
    inner: S,
    state: Authenticator,
}

impl<S, B> Service<axum::http::Request<B>> for AuthMiddleware<S>
where
    S: Service<axum::http::Request<B>, Response = Response> + Send + Clone + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: axum::http::Request<B>) -> Self::Future {
        // Insert the authenticator into request extensions
        req.extensions_mut().insert(self.state.clone());

        let future = self.inner.call(req);
        Box::pin(future)
    }
}
