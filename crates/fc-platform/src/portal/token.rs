//! The authorization_code grant for a PORTAL-plane subject (Go
//! `oauthapi/portal_token.go`, `redeemPortalCode`).
//!
//! `/oauth/token` consumes the code, authenticates the client and checks
//! PKCE as for any code; when the code's subject is a `ptu_…` portal
//! identity it hands over here. The identity must still exist and be ACTIVE
//! and — when the OAuth client fronts a portal app — still hold that app's
//! grant, so a suspension, offboarding or revocation between issuance and
//! redemption bites. Token shapes: an identity-only access token, an
//! id_token minted from the identity with empty roles plus the portal
//! claims, and never a refresh token.

use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use tracing::error;

use super::entity::IdentityStatus;
use super::PortalState;
use crate::auth::auth_service::AuthService;
use crate::auth::authorization_code::AuthorizationCode;
use crate::{Principal, UserScope};

fn oauth_error(status: StatusCode, code: &str, desc: Option<&str>) -> Response {
    let body = match desc {
        Some(d) => json!({ "error": code, "error_description": d }),
        None => json!({ "error": code }),
    };
    (status, Json(body)).into_response()
}

fn server_error() -> Response {
    oauth_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", None)
}

/// Complete the grant for a portal subject; `client_id` is the
/// authenticated OAuth client's public id.
pub async fn redeem_portal_code(
    portal: &PortalState,
    auth_service: &AuthService,
    code: &AuthorizationCode,
    client_id: &str,
) -> Response {
    let ident = match portal.identities.find_by_id(&code.principal_id).await {
        Ok(i) => i,
        Err(e) => {
            error!(error = %e, "portal identity lookup failed");
            return server_error();
        }
    };
    let Some(ident) = ident.filter(|i| i.status == IdentityStatus::Active) else {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            Some("Portal identity not found or suspended"),
        );
    };

    let app = match portal.apps.find_by_oauth_client_id(client_id).await {
        Ok(a) => a,
        Err(e) => {
            error!(error = %e, "portal app lookup failed");
            return server_error();
        }
    };
    if let Some(app) = &app {
        if !app.active || !ident.has_app(&app.id) {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "invalid_grant",
                Some("Portal identity has no access to this portal"),
            );
        }
    }

    // A transient principal-shaped view of the identity: the token
    // generators read id, name, email and updated_at; it never touches the
    // principal store. sub = the ptu_ id.
    let mut synth = Principal::new_user(&ident.email, UserScope::Client);
    synth.id = ident.id.clone();
    synth.name = ident.name.clone();
    synth.updated_at = ident.updated_at;
    synth.all_applications = false;

    // Client-bound, like every interactive identity token.
    let access_token = match auth_service.generate_identity_access_token(&synth, Some(client_id)) {
        Ok(t) => t,
        Err(e) => {
            error!(error = %e, "portal access token mint failed");
            return server_error();
        }
    };
    let has_openid = code
        .scope
        .as_deref()
        .is_some_and(|s| s.split_whitespace().any(|sc| sc == "openid"));
    let id_token = if has_openid {
        match auth_service.generate_portal_id_token(
            &synth,
            &code.client_id,
            code.nonce.clone(),
            &ident.client_id,
            app.as_ref().map(|a| (a.id.as_str(), a.code.as_str())),
        ) {
            Ok(t) => Some(t),
            Err(e) => {
                error!(error = %e, "portal id token mint failed");
                return server_error();
            }
        }
    } else {
        None
    };

    let mut body = json!({
        "access_token": access_token,
        "token_type": "Bearer",
        "expires_in": auth_service.access_token_expiry_secs(),
    });
    if let Some(t) = id_token {
        body["id_token"] = json!(t);
    }
    if let Some(s) = &code.scope {
        body["scope"] = json!(s);
    }
    (
        StatusCode::OK,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::PRAGMA, "no-cache"),
        ],
        Json(body),
    )
        .into_response()
}
