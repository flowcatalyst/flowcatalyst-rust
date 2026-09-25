//! The portal identity side of the password flows (Go
//! `passwordreset/api/api.go`: `SendPortalReset`, `PortalInviteLink`,
//! `SendPortalInvite`, `SendPortalSSOInvite`, `confirmPortalReset`, and
//! `validateToken`'s `portal` flag).
//!
//! Go mints portal reset/invite tokens in the shared
//! `iam_password_reset_tokens` table, keyed by the identity's `ptu_…` id, and
//! the shared confirm/validate endpoints branch on that prefix. Rust keeps
//! the shared handlers in `auth/password_reset_api.rs` untouched: the
//! [`intercept`] layer, applied to that router in `router.rs`, answers every
//! request whose token belongs to a portal identity and passes the rest
//! through. [`confirm_portal_reset`] and [`validate_portal_token`] are the
//! branch bodies, usable from a direct call site as well.
//!
//! The token rows, the password write and the invite bookkeeping are the
//! shared reset-token machinery's infrastructure writes, as in Go (no use
//! case: a credential-set on a confirmed token emits no domain event there
//! either).

use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Request, State},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use super::entity::{is_portal_subject, random_token, IdentityStatus};
use super::repository::{PortalIdentityRepository, PortalResetToken, PortalResetTokenRepository};
use crate::auth::password_service::PasswordService;
use crate::shared::email_service::EmailService;
use crate::shared::error::{PlatformError, Result};

/// The single-use reset token lifetime (Go `resetTokenTTL`).
pub const RESET_TOKEN_TTL_MINUTES: i64 = 15;
/// The first-time set-password lifetime (Go `inviteTokenTTL`).
pub const INVITE_TOKEN_TTL_HOURS: i64 = 72;

/// Lower-case hex SHA-256 of the raw token (the stored `token_hash`).
pub fn hash_token(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

/// Mints and delivers portal set-password invites and reset links, and
/// completes them.
pub struct PortalPasswords {
    pub tokens: Arc<PortalResetTokenRepository>,
    pub identities: Arc<PortalIdentityRepository>,
    pub email_service: Arc<dyn EmailService>,
    pub password_service: Arc<PasswordService>,
    /// Base for the links (the SPA's `/auth/set-password` and
    /// `/auth/reset-password` pages).
    pub external_base_url: String,
}

impl PortalPasswords {
    fn link(&self, page: &str, raw: &str) -> String {
        format!(
            "{}/auth/{page}?token={raw}",
            self.external_base_url.trim_end_matches('/')
        )
    }

    /// Invalidate the identity's outstanding tokens and mint a fresh 72h
    /// invite token (with the optional post-set-password redirect). Returns
    /// the set-password link.
    async fn mint_invite_link(
        &self,
        identity_id: &str,
        redirect_uri: Option<&str>,
    ) -> Result<String> {
        self.tokens.delete_for_subject(identity_id).await?;
        let raw = random_token(32);
        let expires = Utc::now() + Duration::hours(INVITE_TOKEN_TTL_HOURS);
        self.tokens
            .issue(
                identity_id,
                &hash_token(&raw),
                expires,
                "invite",
                redirect_uri,
            )
            .await?;
        Ok(self.link("set-password", &raw))
    }

    /// Mint an invite WITHOUT emailing it (the portal sends its own) and
    /// report when it expires (Go `PortalInviteLink`).
    pub async fn portal_invite_link(
        &self,
        identity_id: &str,
        redirect_uri: Option<&str>,
    ) -> Result<(String, DateTime<Utc>)> {
        let expires = Utc::now() + Duration::hours(INVITE_TOKEN_TTL_HOURS);
        let link = self.mint_invite_link(identity_id, redirect_uri).await?;
        Ok((link, expires))
    }

    /// Mint the invite and email it through the platform mailer (Go
    /// `SendPortalInvite`); a delivery failure is an error.
    pub async fn send_portal_invite(
        &self,
        identity_id: &str,
        email: &str,
        redirect_uri: Option<&str>,
    ) -> Result<DateTime<Utc>> {
        let expires = Utc::now() + Duration::hours(INVITE_TOKEN_TTL_HOURS);
        let link = self.mint_invite_link(identity_id, redirect_uri).await?;
        self.email_service
            .send(&super::email::invite_link(email, &link))
            .await
            .map_err(PlatformError::internal)?;
        Ok(expires)
    }

    /// Email the SSO-org invite: no token, there is no password to set.
    pub async fn send_portal_sso_invite(&self, email: &str, portal_url: &str) -> Result<()> {
        self.email_service
            .send(&super::email::sso_invite(email, portal_url))
            .await
            .map_err(PlatformError::internal)
    }

    /// Mint a 15-minute reset token for the identity and email the neutral
    /// portal reset link (Go `SendPortalReset`).
    pub async fn send_portal_reset(
        &self,
        identity_id: &str,
        email: &str,
        redirect_uri: Option<&str>,
    ) -> Result<()> {
        self.tokens.delete_for_subject(identity_id).await?;
        let raw = random_token(32);
        let expires = Utc::now() + Duration::minutes(RESET_TOKEN_TTL_MINUTES);
        self.tokens
            .issue(
                identity_id,
                &hash_token(&raw),
                expires,
                "reset",
                redirect_uri,
            )
            .await?;
        let link = self.link("reset-password", &raw);
        self.email_service
            .send(&super::email::reset_link(email, &link))
            .await
            .map_err(PlatformError::internal)
    }
}

// ── The shared endpoints' portal branch ──────────────────────────────────

/// Go `validateTokenResponse` (the portal-token shapes).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateTokenResponse {
    pub valid: bool,
    pub reason: Option<String>,
    pub requires_factor: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub portal: bool,
}

/// Go `confirmResponse` (the portal shape).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmResponse {
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub portal: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uri: Option<String>,
}

fn coded(status: StatusCode, code: &str, message: &str) -> Response {
    PlatformError::Coded {
        status,
        code: code.to_string(),
        message: message.to_string(),
        details: Default::default(),
    }
    .into_response()
}

/// `GET /auth/password-reset/validate` for a portal token: valid tokens say
/// `portal: true` so the set-password page shows portal framing.
pub fn validate_portal_token(token: &PortalResetToken) -> Response {
    let body = if token.is_expired() {
        ValidateTokenResponse {
            valid: false,
            reason: Some("expired".to_string()),
            requires_factor: false,
            portal: false,
        }
    } else {
        ValidateTokenResponse {
            valid: true,
            reason: None,
            requires_factor: false,
            portal: true,
        }
    };
    Json(body).into_response()
}

/// `POST /auth/password-reset/confirm` for a portal token (Go
/// `confirmReset`'s expiry check + `confirmPortalReset`): the password
/// policy, hash, write, burn the token set. Portal identities have no 2FA;
/// the response carries the invite's validated redirect.
pub async fn confirm_portal_reset(
    passwords: &PortalPasswords,
    token: &PortalResetToken,
    password: &str,
) -> Response {
    if token.is_expired() {
        let _ = passwords
            .tokens
            .delete_for_subject(&token.principal_id)
            .await;
        return coded(
            StatusCode::BAD_REQUEST,
            "EXPIRED_TOKEN",
            "Reset token has expired.",
        );
    }
    let ident = match passwords.identities.find_by_id(&token.principal_id).await {
        Ok(i) => i,
        Err(e) => return e.into_response(),
    };
    let Some(ident) = ident.filter(|i| i.status == IdentityStatus::Active) else {
        return coded(
            StatusCode::BAD_REQUEST,
            "INVALID_TOKEN",
            "Invalid or expired reset token.",
        );
    };
    if let Some(v) = super::policy::validate(password, &ident.email, &ident.name) {
        return coded(StatusCode::BAD_REQUEST, v.code, &v.message);
    }
    let hash = match passwords.password_service.rehash_password(password) {
        Ok(h) => h,
        Err(e) => return e.into_response(),
    };
    if let Err(e) = passwords
        .identities
        .set_password_hash(&ident.id, &hash)
        .await
    {
        return e.into_response();
    }
    if let Err(e) = passwords.tokens.delete_for_subject(&ident.id).await {
        warn!(identity = %ident.id, error = %e, "failed to clear consumed portal reset tokens");
    }
    info!(identity = %ident.id, "portal password set");
    // The account holder learns their credential changed (best-effort).
    if let Err(e) = passwords
        .email_service
        .send(&super::email::password_changed(&ident.email))
        .await
    {
        warn!(error = %e, "portal password-changed notification not delivered");
    }
    Json(ConfirmResponse {
        status: "ok".to_string(),
        message: "Password set successfully.".to_string(),
        portal: true,
        redirect_uri: token.redirect_uri.clone(),
    })
    .into_response()
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

#[derive(Deserialize)]
struct ConfirmBody {
    #[serde(default)]
    token: String,
    #[serde(default)]
    password: String,
}

/// The confirm body's cap (the request is a token and a password).
const CONFIRM_BODY_LIMIT: usize = 64 * 1024;

/// Middleware over the shared `/auth/password-reset` router: answers
/// `validate` and `confirm` for tokens whose subject is a portal identity and
/// forwards everything else unchanged.
pub async fn intercept(
    State(passwords): State<Arc<PortalPasswords>>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if req.method() == Method::GET && path.ends_with("/validate") {
        let raw = axum::extract::Query::<TokenQuery>::try_from_uri(req.uri())
            .ok()
            .and_then(|q| q.0.token)
            .unwrap_or_default();
        if let Some(token) = portal_token(&passwords, &raw).await {
            return validate_portal_token(&token);
        }
        return next.run(req).await;
    }
    if req.method() == Method::POST && path.ends_with("/confirm") {
        let (parts, body) = req.into_parts();
        let bytes = match axum::body::to_bytes(body, CONFIRM_BODY_LIMIT).await {
            Ok(b) => b,
            Err(_) => {
                return coded(
                    StatusCode::BAD_REQUEST,
                    "INVALID_BODY",
                    "malformed request body",
                )
            }
        };
        if let Ok(parsed) = serde_json::from_slice::<ConfirmBody>(&bytes) {
            if let Some(token) = portal_token(&passwords, &parsed.token).await {
                return confirm_portal_reset(&passwords, &token, &parsed.password).await;
            }
        }
        return next
            .run(Request::from_parts(parts, Body::from(bytes)))
            .await;
    }
    next.run(req).await
}

/// The token row for `raw`, when its subject is a portal identity.
async fn portal_token(passwords: &PortalPasswords, raw: &str) -> Option<PortalResetToken> {
    if raw.is_empty() {
        return None;
    }
    match passwords.tokens.find_by_hash(&hash_token(raw)).await {
        Ok(Some(t)) if is_portal_subject(&t.principal_id) => Some(t),
        Ok(_) => None,
        Err(e) => {
            warn!(error = %e, "portal reset token lookup failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_hash_is_hex_sha256() {
        assert_eq!(
            hash_token("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn response_shapes_follow_go() {
        let valid = ValidateTokenResponse {
            valid: true,
            reason: None,
            requires_factor: false,
            portal: true,
        };
        assert_eq!(
            serde_json::to_value(valid).unwrap(),
            serde_json::json!({"valid": true, "reason": null, "requiresFactor": false, "portal": true})
        );
        let confirm = ConfirmResponse {
            status: "ok".into(),
            message: "Password set successfully.".into(),
            portal: true,
            redirect_uri: None,
        };
        assert_eq!(
            serde_json::to_value(confirm).unwrap(),
            serde_json::json!({"status": "ok", "message": "Password set successfully.", "portal": true})
        );
    }
}
