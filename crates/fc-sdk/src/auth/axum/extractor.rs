//! `Principal` extractor + three guard-style extractors that gate
//! handlers on auth presence and respond appropriately on miss:
//!
//!   - [`Principal`]          — opt-in; returns 401 JSON if missing.
//!   - [`RequireSession`]     — wraps Principal; on miss issues a 302 to
//!                              the configured login route. Use on browser
//!                              routes.
//!   - [`RequireBearer`]      — wraps Principal; on miss issues 401 JSON.
//!                              Use on API routes.
//!   - [`RequireAuth`]        — wraps Principal; on miss issues a 302 for
//!                              browsers (`Accept: text/html`) and 401 for
//!                              everything else. Use when you need both.

use axum::{
    extract::FromRequestParts,
    http::{header::ACCEPT, request::Parts, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
};

use super::principal::Principal as AxumPrincipal;
use super::state::AuthState;
use axum::http::Method;

/// Extracts the authenticated [`Principal`](super::Principal) from a request.
/// Returns 401 JSON if the request is unauthenticated. For browser-friendly
/// redirects, use [`RequireSession`] or [`RequireAuth`].
pub struct Principal(pub AxumPrincipal);

impl<S> FromRequestParts<S> for Principal
where
    S: Send + Sync,
{
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if let Some(Some(p)) = parts.extensions.get::<Option<AxumPrincipal>>() {
            Ok(Principal(p.clone()))
        } else {
            Err(AuthRejection::Unauthorized)
        }
    }
}

/// Extracts the principal, redirecting browsers to `/auth/login` on miss.
pub struct RequireSession(pub AxumPrincipal);

impl<S> FromRequestParts<S> for RequireSession
where
    S: Send + Sync,
{
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if let Some(Some(p)) = parts.extensions.get::<Option<AxumPrincipal>>() {
            return Ok(RequireSession(p.clone()));
        }
        let state = parts
            .extensions
            .get::<AuthState>()
            .cloned()
            .ok_or(AuthRejection::Misconfigured)?;
        let return_to = parts
            .uri
            .path_and_query()
            .map(|p| p.to_string())
            .unwrap_or_default();
        Err(AuthRejection::Redirect(build_login_redirect(
            &state, &return_to,
        )))
    }
}

/// Extracts the principal, 401-JSON on miss.
pub struct RequireBearer(pub AxumPrincipal);

impl<S> FromRequestParts<S> for RequireBearer
where
    S: Send + Sync,
{
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if let Some(Some(p)) = parts.extensions.get::<Option<AxumPrincipal>>() {
            Ok(RequireBearer(p.clone()))
        } else {
            Err(AuthRejection::Unauthorized)
        }
    }
}

/// Extracts the principal, redirecting browsers and 401-ing machines on miss.
pub struct RequireAuth(pub AxumPrincipal);

impl<S> FromRequestParts<S> for RequireAuth
where
    S: Send + Sync,
{
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if let Some(Some(p)) = parts.extensions.get::<Option<AxumPrincipal>>() {
            return Ok(RequireAuth(p.clone()));
        }
        if wants_html(&parts.headers)
            && (parts.method == Method::GET || parts.method == Method::HEAD)
        {
            let state = parts
                .extensions
                .get::<AuthState>()
                .cloned()
                .ok_or(AuthRejection::Misconfigured)?;
            let return_to = parts
                .uri
                .path_and_query()
                .map(|p| p.to_string())
                .unwrap_or_default();
            return Err(AuthRejection::Redirect(build_login_redirect(
                &state, &return_to,
            )));
        }
        Err(AuthRejection::Unauthorized)
    }
}

fn wants_html(headers: &HeaderMap) -> bool {
    headers
        .get(ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.contains("text/html"))
}

fn build_login_redirect(state: &AuthState, return_to: &str) -> String {
    format!(
        "{}?{}={}",
        state.routes.login,
        state.routes.return_to_param,
        urlencoding::encode(return_to),
    )
}

/// Rejection type used by all auth extractors. `IntoResponse` produces
/// either a redirect or a `401 Unauthorized` JSON body.
pub enum AuthRejection {
    Unauthorized,
    Redirect(String),
    Misconfigured,
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        match self {
            AuthRejection::Unauthorized => {
                let body = axum::Json(serde_json::json!({ "error": "unauthorized" }));
                let mut resp = (StatusCode::UNAUTHORIZED, body).into_response();
                resp.headers_mut().insert(
                    "WWW-Authenticate",
                    r#"Bearer realm="flowcatalyst""#.parse().unwrap(),
                );
                resp
            }
            AuthRejection::Redirect(loc) => Redirect::to(&loc).into_response(),
            AuthRejection::Misconfigured => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "fc auth middleware not registered on this router",
            )
                .into_response(),
        }
    }
}
