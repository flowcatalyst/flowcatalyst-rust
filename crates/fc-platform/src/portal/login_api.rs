//! `/portal/*` — the portal plane's front-channel login surface (Go
//! `portalauth/endpoints.go` and the bridge's `RegisterPortalRoutes`):
//! `GET /portal/authorize`, `POST /portal/auth/check-domain`,
//! `POST /portal/auth/login`, `POST /portal/auth/password-reset` and
//! `GET /portal/auth/oidc/login`.
//!
//! Public (like `/auth/login`): these endpoints authenticate portal
//! identities themselves and never read or write `fc_session`; the portal app
//! runs its own session from the id_token. Login-flow rows, OIDC states and
//! authorization codes are auth-flow infrastructure, written directly (Go
//! writes them outside the unit of work too).

use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use tracing::warn;

use super::entity::{email_domain_of, normalize_email, random_token, LoginFlow};
use super::PortalState;
use crate::auth::authorization_code::{AuthorizationCode, Pkce};
use crate::auth::oidc_login_api::OidcLoginApiState;
use crate::shared::error::PlatformError;
use crate::shared::rate_limit_store::{Bucket, RateLimitDecision};

/// Go `ratelimit.BucketPortalLogin`.
pub const BUCKET_PORTAL_LOGIN: Bucket = Bucket("portal_login");

const FLOW_EXPIRED: &str = "The login flow has expired — return to the portal and try again";

/// The portal login routes' state: the plane, plus the OIDC bridge's
/// settings for the SSO start.
#[derive(Clone)]
pub struct PortalLoginState {
    pub portal: PortalState,
    pub oidc: OidcLoginApiState,
}

// ── response helpers ──────────────────────────────────────────────────────

/// Go's `url.QueryEscape`: unreserved bytes kept, space as `+`, the rest
/// percent-encoded in upper case.
pub fn query_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn with_query(base: &str, query: &str) -> String {
    let sep = if base.contains('?') { '&' } else { '?' };
    format!("{base}{sep}{query}")
}

/// The RFC 6749 `{error, error_description}` direct failure.
fn oauth_error(status: StatusCode, code: &str, desc: &str) -> Response {
    (
        status,
        Json(json!({ "error": code, "error_description": desc })),
    )
        .into_response()
}

/// Bounce back to the (validated) redirect URI with OAuth error params.
fn error_redirect(redirect_uri: &str, code: &str, desc: &str, state: &str) -> Response {
    let url = with_query(
        redirect_uri,
        &format!(
            "error={}&error_description={}&state={}",
            query_escape(code),
            query_escape(desc),
            query_escape(state)
        ),
    );
    redirect(StatusCode::TEMPORARY_REDIRECT, url)
}

pub(crate) fn redirect(status: StatusCode, location: String) -> Response {
    (status, [(header::LOCATION, location)]).into_response()
}

pub(crate) fn coded(status: StatusCode, code: &str, message: &str) -> Response {
    PlatformError::Coded {
        status,
        code: code.to_string(),
        message: message.to_string(),
        details: Default::default(),
    }
    .into_response()
}

/// The `{code, message}` bodies of the password endpoint (Go writes these
/// by hand, without the `error` key).
fn code_message(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({ "code": code, "message": message }))).into_response()
}

fn invalid_credentials() -> Response {
    code_message(
        StatusCode::UNAUTHORIZED,
        "INVALID_CREDENTIALS",
        "Invalid email or password",
    )
}

fn flow_expired() -> Response {
    coded(StatusCode::BAD_REQUEST, "FLOW_EXPIRED", FLOW_EXPIRED)
}

fn decode<T: DeserializeOwned>(raw: &Bytes) -> Result<T, Response> {
    serde_json::from_slice(raw).map_err(|_| {
        coded(
            StatusCode::BAD_REQUEST,
            "INVALID_BODY",
            "malformed request body",
        )
    })
}

/// The per-(client, email) budget; fails open on a backend error.
async fn over_budget(portal: &PortalState, key: &str) -> Option<Response> {
    match portal
        .rate_limit_store
        .check_and_record(BUCKET_PORTAL_LOGIN, key, portal.portal_login_policy)
        .await
    {
        Ok(RateLimitDecision::Reject { retry_after_secs }) => Some(
            PlatformError::TooManyRequests {
                retry_after_secs: retry_after_secs.max(1),
                message: "too many attempts".to_string(),
            }
            .into_response(),
        ),
        Ok(RateLimitDecision::Allow) => None,
        Err(e) => {
            warn!(error = %e, "portal login rate-limit backend error; failing open");
            None
        }
    }
}

// ── GET /portal/authorize ─────────────────────────────────────────────────

/// The portal plane's authorization entry. It mirrors `/oauth/authorize`'s
/// client/redirect/PKCE validation but never consults a session: it parks
/// the chain in a login flow and bounces to the SPA portal login page.
pub async fn authorize(
    State(s): State<PortalLoginState>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let param = |k: &str| q.get(k).map(String::as_str).unwrap_or("");
    let (response_type, client_id, redirect_uri) = (
        param("response_type"),
        param("client_id"),
        param("redirect_uri"),
    );
    let (scope, state_param, nonce) = (param("scope"), param("state"), param("nonce"));
    let (challenge, method) = (param("code_challenge"), param("code_challenge_method"));

    if state_param.trim().is_empty() {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "`state` parameter is required for CSRF protection",
        );
    }
    // Client + redirect_uri validation before any redirect (RFC 6749
    // §4.1.2.1).
    let client = match s.portal.portal_oauth.find_by_client_id(client_id).await {
        Err(_) => {
            return oauth_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "Internal error",
            )
        }
        Ok(None) => {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "unauthorized_client",
                "Unknown or inactive client",
            )
        }
        Ok(Some(c)) if !c.active => {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "unauthorized_client",
                "Unknown or inactive client",
            )
        }
        Ok(Some(c)) => c,
    };
    let Some(portal_client_id) = client.portal_client_id.clone().filter(|p| !p.is_empty()) else {
        // The portal plane serves only portal-flagged clients.
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "unauthorized_client",
            "Client is not a portal client",
        );
    };
    if !crate::auth::oauth_api::matches_redirect_uri(redirect_uri, &client.redirect_uris) {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Invalid redirect_uri",
        );
    }

    // redirect_uri validated — errors may now bounce back.
    if response_type != "code" {
        return error_redirect(
            redirect_uri,
            "unsupported_response_type",
            "Only 'code' response type is supported",
            state_param,
        );
    }
    if client.pkce_required && challenge.is_empty() {
        return error_redirect(
            redirect_uri,
            "invalid_request",
            "PKCE code_challenge is required",
            state_param,
        );
    }
    if !method.is_empty() && method != "S256" {
        return error_redirect(
            redirect_uri,
            "invalid_request",
            "Only the S256 code_challenge_method is supported",
            state_param,
        );
    }

    let mut flow = LoginFlow::new(client_id, &portal_client_id, redirect_uri, state_param);
    let opt = |v: &str| (!v.is_empty()).then(|| v.to_string());
    flow.scope = opt(scope);
    flow.nonce = opt(nonce);
    flow.code_challenge = opt(challenge);
    if !challenge.is_empty() {
        // Never store a challenge without its method.
        flow.code_challenge_method =
            Some(if method.is_empty() { "S256" } else { method }.to_string());
    }
    if s.portal.flows.park(&flow).await.is_err() {
        return error_redirect(
            redirect_uri,
            "server_error",
            "Could not start the login flow",
            state_param,
        );
    }
    redirect(
        StatusCode::TEMPORARY_REDIRECT,
        format!(
            "{}?flow={}",
            s.portal.login_page_path,
            query_escape(&flow.id)
        ),
    )
}

// ── POST /portal/auth/check-domain ────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FlowEmailBody {
    #[serde(default)]
    flow_id: String,
    #[serde(default)]
    email: String,
}

/// `{flowId, email}`: a domain owned by an OIDC IdP → SSO (with the redirect
/// that starts it); anything else → PASSWORD. Never reveals whether the
/// identity exists.
pub async fn check_domain(State(s): State<PortalLoginState>, raw: Bytes) -> Response {
    let body: FlowEmailBody = match decode(&raw) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let flow = match s.portal.flows.find_live(&body.flow_id).await {
        Ok(Some(f)) => f,
        Ok(None) => return flow_expired(),
        Err(e) => return e.into_response(),
    };
    let domain = email_domain_of(&body.email);
    if domain.is_empty() {
        return coded(
            StatusCode::BAD_REQUEST,
            "EMAIL_INVALID",
            "email is not valid",
        );
    }
    match s.portal.oidc_provider_for_domain(&domain).await {
        Err(e) => e.into_response(),
        Ok(Some(idp)) => Json(json!({
            "method": "SSO",
            "redirectUrl": format!(
                "/portal/auth/oidc/login?flow={}&provider_id={}",
                query_escape(&flow.id),
                query_escape(&idp.id)
            ),
        }))
        .into_response(),
        Ok(None) => Json(json!({ "method": "PASSWORD" })).into_response(),
    }
}

// ── POST /portal/auth/login ───────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginBody {
    #[serde(default)]
    flow_id: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    password: String,
}

/// `{flowId, email, password}`. On success it consumes the flow, mints the
/// authorization code with the portal-identity subject and answers with the
/// code redirect. Failures are a uniform INVALID_CREDENTIALS (no account or
/// status enumeration) and do NOT consume the flow.
pub async fn password_login(State(s): State<PortalLoginState>, raw: Bytes) -> Response {
    let body: LoginBody = match decode(&raw) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let portal = &s.portal;
    let flow = match portal.flows.find_live(&body.flow_id).await {
        Ok(Some(f)) => f,
        Ok(None) => return flow_expired(),
        Err(e) => return e.into_response(),
    };

    // Brute-force ceiling per (client, email): the plane has no lockout
    // table; the limiter is the control.
    let key = format!("{}:{}", flow.portal_client_id, normalize_email(&body.email));
    if let Some(rejected) = over_budget(portal, &key).await {
        return rejected;
    }

    // A domain owned by an IdP never authenticates by password — the org's
    // IdP is the authority.
    if let Ok(Some(_)) = portal
        .oidc_provider_for_domain(&email_domain_of(&body.email))
        .await
    {
        return code_message(
            StatusCode::UNAUTHORIZED,
            "SSO_REQUIRED",
            "Sign in with your organisation account",
        );
    }

    let ident = match portal
        .identities
        .find_by_client_and_email(&flow.portal_client_id, &body.email)
        .await
    {
        Ok(i) => i,
        Err(e) => return e.into_response(),
    };
    let Some(ident) = ident.filter(|i| i.can_sign_in_with_password()) else {
        // Unknown, suspended and password-less identities are
        // indistinguishable: burn comparable time and refuse.
        equalize_timing(portal, &body.password);
        return invalid_credentials();
    };
    let hash = ident.password_hash.as_deref().unwrap_or_default();
    if !matches!(
        portal
            .password_service
            .verify_password(&body.password, hash),
        Ok(true)
    ) {
        return invalid_credentials();
    }

    // App gate — after the password check, so it reveals nothing to someone
    // who cannot prove the credential.
    match portal
        .apps
        .find_by_oauth_client_id(&flow.oauth_client_id)
        .await
    {
        Err(e) => return e.into_response(),
        Ok(Some(app)) if !app.active || !ident.has_app(&app.id) => {
            return code_message(
                StatusCode::FORBIDDEN,
                "NO_PORTAL_ACCESS",
                "You don't have access to this portal",
            );
        }
        Ok(_) => {}
    }

    // Success: single-use the flow, then mint the code.
    let consumed = match portal.flows.consume(&flow.id).await {
        Ok(Some(f)) => f,
        _ => return flow_expired(),
    };
    let redirect_url = match issue_code(portal, &consumed, &ident.id).await {
        Ok(u) => u,
        Err(_) => {
            return coded(
                StatusCode::INTERNAL_SERVER_ERROR,
                "CODE",
                "could not issue the authorization code",
            )
        }
    };
    let _ = portal.identities.touch_last_login(&ident.id).await;
    Json(json!({ "redirectUrl": redirect_url })).into_response()
}

/// Verify against a fixed hash so a refused unknown account costs what a
/// wrong password costs (Go `passwordhash.EqualizeTiming`).
fn equalize_timing(portal: &PortalState, password: &str) {
    static DUMMY: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let dummy = DUMMY.get_or_init(|| {
        portal
            .password_service
            .rehash_password("equalize-timing")
            .ok()
    });
    if let Some(hash) = dummy {
        let _ = portal.password_service.verify_password(password, hash);
    }
}

/// Mint the authorization code for a consumed flow and return the full
/// redirect URL (shared by the password login and the SSO sink).
pub async fn issue_code(
    portal: &PortalState,
    flow: &LoginFlow,
    subject_id: &str,
) -> Result<String, PlatformError> {
    let raw = random_token(48);
    let pkce = match &flow.code_challenge {
        Some(challenge) => Pkce::from_parts(
            Some(challenge.clone()),
            flow.code_challenge_method.as_deref(),
        )
        .map_err(|_| PlatformError::internal("stored code_challenge_method is not S256"))?,
        None => None,
    };
    let code = AuthorizationCode {
        scope: flow.scope.clone(),
        nonce: flow.nonce.clone(),
        state: Some(flow.state.clone()),
        ..AuthorizationCode::new(
            raw.clone(),
            flow.oauth_client_id.clone(),
            subject_id.to_string(),
            flow.redirect_uri.clone(),
        )
    }
    .with_pkce(pkce);
    portal.auth_codes.insert(&code).await?;
    Ok(with_query(
        &flow.redirect_uri,
        &format!(
            "code={}&state={}",
            query_escape(&raw),
            query_escape(&flow.state)
        ),
    ))
}

// ── POST /portal/auth/password-reset ──────────────────────────────────────

const RESET_MESSAGE: &str = "If an account exists, a reset email has been sent.";

/// `{flowId, email}`: the portal forgot-password flow. Silent success —
/// only an existing ACTIVE identity in the flow's client is mailed; the link
/// leads back to the portal's origin.
pub async fn request_password_reset(State(s): State<PortalLoginState>, raw: Bytes) -> Response {
    let body: FlowEmailBody = match decode(&raw) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let portal = &s.portal;
    let flow = match portal.flows.find_live(&body.flow_id).await {
        Ok(Some(f)) => f,
        Ok(None) => return flow_expired(),
        Err(e) => return e.into_response(),
    };
    let email = normalize_email(&body.email);
    let domain = email_domain_of(&email);
    if domain.is_empty() {
        return coded(
            StatusCode::BAD_REQUEST,
            "EMAIL_INVALID",
            "email is not valid",
        );
    }
    // Same ceiling as the login, on a distinct sub-key.
    let key = format!("reset:{}:{}", flow.portal_client_id, email);
    if let Some(rejected) = over_budget(portal, &key).await {
        return rejected;
    }
    let message = || Json(json!({ "message": RESET_MESSAGE })).into_response();
    // SSO-owned domains never get password resets.
    if let Ok(Some(_)) = portal.oidc_provider_for_domain(&domain).await {
        return message();
    }
    if let Ok(Some(ident)) = portal
        .identities
        .find_by_client_and_email(&flow.portal_client_id, &email)
        .await
    {
        if ident.status == super::entity::IdentityStatus::Active {
            let redirect = super::api::origin_of(&flow.redirect_uri);
            if let Err(e) = portal
                .passwords
                .send_portal_reset(&ident.id, &ident.email, redirect.as_deref())
                .await
            {
                // Suppressed (anti-enumeration).
                warn!(error = %e, "portal password reset not delivered");
            }
        }
    }
    message()
}

// ── GET /portal/auth/oidc/login ───────────────────────────────────────────

/// Start a portal-plane IdP handshake: consume the flow (single use),
/// require an OIDC provider — any OIDC IdP may serve any portal; which
/// client's identity the login yields comes from the flow — and park the
/// flow's OAuth chain on a portal-flagged OIDC state.
pub async fn portal_oidc_login(
    State(s): State<PortalLoginState>,
    headers: HeaderMap,
    uri: Uri,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let flow_id = q.get("flow").map(String::as_str).unwrap_or("");
    let provider_id = q.get("provider_id").map(String::as_str).unwrap_or("");
    if flow_id.is_empty() || provider_id.is_empty() {
        return coded(
            StatusCode::BAD_REQUEST,
            "MISSING_PARAM",
            "flow and provider_id are required",
        );
    }
    let flow = match s.portal.flows.consume(flow_id).await {
        Ok(Some(f)) => f,
        Ok(None) => return flow_expired(),
        Err(e) => return e.into_response(),
    };
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost")
        .to_string();
    super::oidc::start(&s, &flow, provider_id, &host, &uri).await
}

/// The per-IP quota on the portal routes: Go mounts them in the OIDC
/// bridge's governor group, `FC_OIDC_RATE_PER_MIN` (60) and `FC_OIDC_BURST`
/// (30) (`ratelimit.OIDCBridgeGovernorFromEnv`).
pub fn portal_ip_rate_config() -> crate::shared::rate_limit_middleware::RateLimitConfig {
    let read = |name: &str, default: u32| {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .and_then(std::num::NonZeroU32::new)
            .unwrap_or(std::num::NonZeroU32::new(default).expect("non-zero default"))
    };
    crate::shared::rate_limit_middleware::RateLimitConfig {
        per_minute: read("FC_OIDC_RATE_PER_MIN", 60),
        burst: read("FC_OIDC_BURST", 30),
    }
}

/// `/portal` routes (public; per-IP limited by the router).
pub fn portal_login_router(state: PortalLoginState) -> Router {
    Router::new()
        .route("/authorize", get(authorize))
        .route("/auth/check-domain", post(check_domain))
        .route("/auth/login", post(password_login))
        .route("/auth/password-reset", post(request_password_reset))
        .route("/auth/oidc/login", get(portal_oidc_login))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_escape_matches_go() {
        assert_eq!(query_escape("a b&c=d/é~"), "a+b%26c%3Dd%2F%C3%A9~");
        assert_eq!(query_escape("Only 'code'"), "Only+%27code%27");
    }

    #[test]
    fn redirect_urls_keep_existing_queries() {
        assert_eq!(with_query("https://x/cb", "a=1"), "https://x/cb?a=1");
        assert_eq!(
            with_query("https://x/cb?t=2", "a=1"),
            "https://x/cb?t=2&a=1"
        );
    }
}
