//! Portal-plane SSO (Go bridge `handlePortalOIDCLogin` and
//! `handlePortalCallback`).
//!
//! The start (`GET /portal/auth/oidc/login`) parks the portal flow's OAuth
//! chain on a portal-flagged OIDC login state. The IdP returns to the shared
//! `/auth/oidc/callback` (the redirect URI registered at every IdP); the
//! [`intercept`] layer, applied to the OIDC router in `router.rs`, claims the
//! callbacks whose state is portal-flagged and completes them here — JIT
//! portal identity, code issuance with the portal subject, never
//! `fc_session` — and passes every other callback through untouched.

use axum::{
    extract::{Request, State},
    http::{header, Method, StatusCode, Uri},
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::{Duration, Utc};
use std::collections::HashMap;

use super::entity::{email_domain_of, IdentityStatus, LoginFlow};
use super::login_api::{coded, issue_code, query_escape, redirect, PortalLoginState};
use super::operations::{EnsureCommand, EnsurePortalIdentityUseCase};
use super::repository::PortalOidcState;
use super::PortalState;
use crate::auth::oidc_login_api::{portal_handshake, portal_verify_callback};
use crate::identity_provider::entity::IdentityProviderType;
use crate::usecase::{ExecutionContext, UseCase};

/// How long a parked OIDC login state lives (the OIDC bridge's TTL).
const STATE_TTL_SECONDS: i64 = 600;

fn resolve_failed() -> Response {
    coded(
        StatusCode::INTERNAL_SERVER_ERROR,
        "OIDC_RESOLVE_FAILED",
        "OIDC could not be initialised for this provider",
    )
}

/// Go `Bridge.ResolveByProviderID`'s guards, all fail-closed: the IdP must
/// exist, be OIDC, be fully configured, and a multi-tenant one must bound
/// the emails it may assert.
async fn resolve_provider(
    portal: &PortalState,
    provider_id: &str,
) -> Option<crate::IdentityProvider> {
    let idp = portal
        .identity_providers
        .find_by_id(provider_id)
        .await
        .ok()
        .flatten()?;
    let usable = idp.r#type == IdentityProviderType::Oidc
        && idp.oidc_issuer_url.is_some()
        && idp.oidc_client_id.is_some()
        && !(idp.oidc_multi_tenant && idp.allowed_email_domains.is_empty());
    usable.then_some(idp)
}

/// Start the IdP handshake for a consumed portal flow.
pub async fn start(
    s: &PortalLoginState,
    flow: &LoginFlow,
    provider_id: &str,
    host: &str,
    uri: &Uri,
) -> Response {
    let Some(idp) = resolve_provider(&s.portal, provider_id).await else {
        return resolve_failed();
    };
    let handshake = portal_handshake(&s.oidc, host, uri, &idp);
    let parked = PortalOidcState {
        state: handshake.state,
        identity_provider_id: idp.id.clone(),
        nonce: handshake.nonce,
        code_verifier: handshake.code_verifier,
        portal_client_id: Some(flow.portal_client_id.clone()),
        oauth_client_id: Some(flow.oauth_client_id.clone()),
        oauth_redirect_uri: Some(flow.redirect_uri.clone()),
        oauth_scope: flow.scope.clone(),
        oauth_state: Some(flow.state.clone()),
        oauth_code_challenge: flow.code_challenge.clone(),
        oauth_code_challenge_method: flow.code_challenge_method.clone(),
        oauth_nonce: flow.nonce.clone(),
    };
    let expires = Utc::now() + Duration::seconds(STATE_TTL_SECONDS);
    if s.portal.oidc_states.park(&parked, expires).await.is_err() {
        return coded(
            StatusCode::INTERNAL_SERVER_ERROR,
            "OIDC_STATE",
            "persist state failed",
        );
    }
    redirect(StatusCode::FOUND, handshake.authorize_url)
}

/// Middleware over the shared OIDC router: completes callbacks whose state
/// is a portal-plane handshake and forwards the rest.
pub async fn intercept(State(s): State<PortalLoginState>, req: Request, next: Next) -> Response {
    if req.method() != Method::GET || !req.uri().path().ends_with("/oidc/callback") {
        return next.run(req).await;
    }
    let query: HashMap<String, String> =
        axum::extract::Query::<HashMap<String, String>>::try_from_uri(req.uri())
            .map(|q| q.0)
            .unwrap_or_default();
    let (Some(state_param), Some(code)) = (
        query.get("state").filter(|v| !v.is_empty()),
        query.get("code").filter(|v| !v.is_empty()),
    ) else {
        return next.run(req).await;
    };
    let parked = match s.portal.oidc_states.consume_portal(state_param).await {
        Ok(Some(p)) => p,
        Ok(None) => return next.run(req).await,
        Err(e) => return e.into_response(),
    };
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost")
        .to_string();
    callback(&s, &parked, code, &host, req.uri()).await
}

/// Verify the IdP callback of a portal handshake and hand it to the sink.
async fn callback(
    s: &PortalLoginState,
    parked: &PortalOidcState,
    code: &str,
    host: &str,
    uri: &Uri,
) -> Response {
    let Some(idp) = resolve_provider(&s.portal, &parked.identity_provider_id).await else {
        return resolve_failed();
    };
    let verified = portal_verify_callback(
        &s.oidc,
        host,
        uri,
        &idp,
        code,
        &parked.code_verifier,
        &parked.nonce,
    )
    .await;
    let (email, name) = match verified {
        Ok(v) => v,
        Err((status, code, message)) => return coded(status, code, &message),
    };
    // Provider-direct trust binding: the IdP's own allowed_email_domains
    // (non-empty for multi-tenant IdPs by the resolve guard).
    let domain = email_domain_of(&email);
    if !idp.allowed_email_domains.is_empty()
        && !idp
            .allowed_email_domains
            .iter()
            .any(|d| d.eq_ignore_ascii_case(&domain))
    {
        return coded(
            StatusCode::FORBIDDEN,
            "EMAIL_DOMAIN_MISMATCH",
            "the token's email domain is not allowed for this identity provider",
        );
    }
    complete(&s.portal, parked, &email, name.as_deref().unwrap_or("")).await
}

/// Bounce the user-agent back to the portal with OAuth error params (the
/// redirect URI was validated at `/portal/authorize`).
fn portal_error_redirect(
    redirect_uri: &str,
    oauth_state: &str,
    code: &str,
    desc: &str,
) -> Response {
    let sep = if redirect_uri.contains('?') { '&' } else { '?' };
    redirect(
        StatusCode::FOUND,
        format!(
            "{redirect_uri}{sep}error={}&error_description={}&state={}",
            query_escape(code),
            query_escape(desc),
            query_escape(oauth_state)
        ),
    )
}

/// The portal sink for a verified IdP callback (Go `handlePortalCallback`):
/// JIT-create the identity for the flow's client on first login, refuse
/// suspended identities and missing app grants, mint the chained code with
/// the portal subject, and redirect.
pub async fn complete(
    portal: &PortalState,
    parked: &PortalOidcState,
    email: &str,
    name: &str,
) -> Response {
    let (Some(oauth_client_id), Some(redirect_uri), Some(oauth_state), Some(portal_client_id)) = (
        parked.oauth_client_id.as_deref(),
        parked.oauth_redirect_uri.as_deref(),
        parked.oauth_state.as_deref(),
        parked.portal_client_id.as_deref(),
    ) else {
        return coded(
            StatusCode::BAD_REQUEST,
            "PORTAL_STATE_INVALID",
            "portal login state is missing its OAuth chain",
        );
    };

    // The app this OAuth client fronts (none = legacy client-wide portal).
    let app = match portal.apps.find_by_oauth_client_id(oauth_client_id).await {
        Ok(a) => a,
        Err(_) => {
            return coded(
                StatusCode::INTERNAL_SERVER_ERROR,
                "PORTAL_APP",
                "portal app lookup failed",
            )
        }
    };
    if app.as_ref().is_some_and(|a| !a.active) {
        return portal_error_redirect(
            redirect_uri,
            oauth_state,
            "access_denied",
            "This portal is not currently available",
        );
    }

    let ident = match portal
        .identities
        .find_by_client_and_email(portal_client_id, email)
        .await
    {
        Ok(i) => i,
        Err(e) => return e.into_response(),
    };
    let ident = match ident {
        None => {
            // First login: JIT-create through the ensure use case (event +
            // audit), as the system actor — the human just authenticated at
            // their org's IdP. It JIT-grants the app it came through.
            let name = name.trim();
            let cmd = EnsureCommand {
                client_id: portal_client_id.to_string(),
                email: email.to_string(),
                name: (!name.is_empty()).then(|| name.to_string()),
                source: "JIT".to_string(),
                portal_app_id: app.as_ref().map(|a| a.id.clone()),
            };
            let use_case = EnsurePortalIdentityUseCase::new(
                portal.identities.clone(),
                portal.apps.clone(),
                portal.clients.clone(),
                portal.unit_of_work.clone(),
            );
            let event = match use_case
                .run(cmd, ExecutionContext::create(""))
                .await
                .into_result()
            {
                Ok(e) => e,
                Err(e) => return crate::shared::error::PlatformError::from(e).into_response(),
            };
            match portal.identities.find_by_id(&event.identity_id).await {
                Ok(Some(i)) => i,
                _ => {
                    return coded(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "IDENTITY",
                        "post-create identity lookup failed",
                    )
                }
            }
        }
        // Suspended stays suspended: an SSO login never self-reactivates.
        Some(i) if i.status != IdentityStatus::Active => {
            return portal_error_redirect(
                redirect_uri,
                oauth_state,
                "access_denied",
                "This account is suspended for this portal",
            );
        }
        // An existing identity is not JIT-granted another of the client's
        // portals — access is the portal's call.
        Some(i) if app.as_ref().is_some_and(|a| !i.has_app(&a.id)) => {
            return portal_error_redirect(
                redirect_uri,
                oauth_state,
                "access_denied",
                "You don't have access to this portal",
            );
        }
        Some(i) => i,
    };

    let mut flow = LoginFlow::new(oauth_client_id, portal_client_id, redirect_uri, oauth_state);
    flow.scope = parked.oauth_scope.clone();
    flow.nonce = parked.oauth_nonce.clone();
    flow.code_challenge = parked.oauth_code_challenge.clone();
    flow.code_challenge_method = parked.oauth_code_challenge_method.clone();
    let redirect_url = match issue_code(portal, &flow, &ident.id).await {
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
    redirect(StatusCode::FOUND, redirect_url)
}
