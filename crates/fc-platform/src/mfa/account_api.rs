//! A signed-in user's own password and sign-in history (Go
//! `auth/login/change_password.go`, `session_history.go`), for the Profile
//! screen:
//!
//! - `POST /auth/change-password` — the current password, and a current
//!   second factor when the user has one
//! - `POST /auth/change-password/send-email-code` — an email PIN for that
//! - `GET /auth/login-history` — the 20 most recent sign-in attempts
//!
//! Like Go, the password change writes the hash directly and emits no
//! domain event (the admin and email-link resets go through
//! `ResetPasswordUseCase`); it revokes remembered devices and refresh
//! tokens and mails the user.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::warn;

use super::entity::MethodType;
use super::login_api::{coded, decode, email_of, unauthorized, TwoFactorLogin};
use crate::shared::middleware::OptionalAuth;
use crate::Principal;

/// What the account routes need beyond the 2FA state.
pub struct AccountState {
    pub two_factor: Arc<TwoFactorLogin>,
    pub password_service: Arc<crate::PasswordService>,
    pub refresh_token_repo: Arc<crate::RefreshTokenRepository>,
}

async fn principal_from_session(
    s: &AccountState,
    auth: &OptionalAuth,
) -> Result<Principal, Box<Response>> {
    // No or stale credential: Go's handler answers its own 401.
    let Some(ctx) = auth.0.as_ref() else {
        return Err(Box::new(unauthorized("Not authenticated")));
    };
    match s
        .two_factor
        .principal_repo
        .find_by_id(&ctx.principal_id)
        .await
    {
        Ok(Some(p)) if p.active => Ok(p),
        _ => Err(Box::new(unauthorized("Not authenticated"))),
    }
}

fn server_error(code: &str, message: &str) -> Response {
    coded(StatusCode::INTERNAL_SERVER_ERROR, code, message)
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
    code: String,
}

/// A code from any of the user's confirmed factors, or a recovery code when
/// they have TOTP (Go `verifyAnySecondFactor`).
async fn verify_any_second_factor(
    s: &TwoFactorLogin,
    p: &Principal,
    confirmed: &[MethodType],
    code: &str,
) -> bool {
    for m in confirmed {
        let ok = match m {
            MethodType::Totp => s.mfa.verify_totp(&p.id, code).await,
            MethodType::EmailPin => s.mfa.verify_login_email_pin(&p.id, code).await,
        };
        if matches!(ok, Ok(true)) {
            return true;
        }
    }
    confirmed.contains(&MethodType::Totp)
        && matches!(s.mfa.verify_recovery_code(&p.id, code).await, Ok(true))
}

/// `POST /auth/change-password` (Go `handleChangePassword`).
async fn change_password(
    State(s): State<Arc<AccountState>>,
    auth: OptionalAuth,
    jar: CookieJar,
    body: Bytes,
) -> Response {
    let p = match principal_from_session(&s, &auth).await {
        Ok(p) => p,
        Err(resp) => return *resp,
    };
    let req: ChangePasswordRequest = match decode(&body) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    let tf = &s.two_factor;
    // A federated account manages its password at the identity provider.
    if tf.sso_managed(&p, None).await {
        return coded(
            StatusCode::BAD_REQUEST,
            "SSO_MANAGED",
            "Your password is managed by your identity provider and cannot be changed here.",
        );
    }
    let Some(hash) = p
        .user_identity
        .as_ref()
        .and_then(|i| i.password_hash.as_deref())
    else {
        return coded(
            StatusCode::BAD_REQUEST,
            "NO_PASSWORD",
            "This account signs in without a password.",
        );
    };
    if !s
        .password_service
        .verify_password(&req.current_password, hash)
        .unwrap_or(false)
    {
        return coded(
            StatusCode::UNAUTHORIZED,
            "INVALID_CURRENT_PASSWORD",
            "Your current password is incorrect.",
        );
    }
    if let Err(e) = s.password_service.validate_password(&req.new_password) {
        return coded(StatusCode::BAD_REQUEST, "PASSWORD_POLICY", &e.to_string());
    }

    // Any confirmed factor means a current code is needed too. The SPA
    // first submits without one and is told which methods to ask for.
    let confirmed = match tf.mfa.confirmed_methods(&p.id).await {
        Ok(c) => c,
        Err(_) => return server_error("MFA_STATUS_FAILED", "could not check two-factor status"),
    };
    if !confirmed.is_empty() {
        if req.code.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "code": "MFA_REQUIRED",
                    "message": "Enter a code from your second factor to change your password.",
                    "methods": confirmed.iter().map(|m| m.as_str()).collect::<Vec<_>>(),
                })),
            )
                .into_response();
        }
        if !verify_any_second_factor(tf, &p, &confirmed, &req.code).await {
            return coded(
                StatusCode::BAD_REQUEST,
                "INVALID_CODE",
                "That code didn't match — try again.",
            );
        }
    }

    let new_hash = match s.password_service.hash_password(&req.new_password) {
        Ok(h) => h,
        Err(_) => return server_error("HASH_FAILED", "could not set the new password"),
    };
    if tf
        .principal_repo
        .update_password_hash(&p.id, &new_hash)
        .await
        .is_err()
    {
        return server_error("UPDATE_FAILED", "could not save the new password");
    }

    // A password change cuts off whoever held the old one: remembered
    // devices and refresh tokens go. Best-effort; the password is changed.
    if let Err(e) = tf.mfa.revoke_all_trusted_devices(&p.id).await {
        warn!(principal_id = %p.id, error = %e, "revoke trusted devices after password change failed");
    }
    let jar = tf.clear_trusted_device_cookie(jar);
    if let Err(e) = s.refresh_token_repo.revoke_all_for_principal(&p.id).await {
        warn!(principal_id = %p.id, error = %e, "revoke refresh tokens after password change failed");
    }
    tf.notifier.password_changed(&email_of(&p)).await;
    (
        jar,
        Json(json!({ "message": "Your password has been changed." })),
    )
        .into_response()
}

/// `POST /auth/change-password/send-email-code`.
async fn send_email_code(State(s): State<Arc<AccountState>>, auth: OptionalAuth) -> Response {
    let p = match principal_from_session(&s, &auth).await {
        Ok(p) => p,
        Err(resp) => return *resp,
    };
    let confirmed = match s.two_factor.mfa.confirmed_methods(&p.id).await {
        Ok(c) => c,
        Err(_) => return server_error("MFA_STATUS_FAILED", "could not check two-factor status"),
    };
    if confirmed.is_empty() {
        return coded(
            StatusCode::BAD_REQUEST,
            "NO_MFA",
            "two-factor is not enabled",
        );
    }
    if !confirmed.contains(&MethodType::EmailPin) {
        return coded(
            StatusCode::BAD_REQUEST,
            "NO_EMAIL_2FA",
            "email codes are not enabled for your account",
        );
    }
    let email = email_of(&p);
    if email.is_empty() {
        return coded(StatusCode::BAD_REQUEST, "NO_EMAIL", "account has no email");
    }
    if s.two_factor
        .mfa
        .send_login_email_pin(&p.id, &email)
        .await
        .is_err()
    {
        return server_error("SEND_FAILED", "could not send the code");
    }
    Json(json!({ "message": "A code has been sent to your email." })).into_response()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginHistoryItem {
    attempt_type: String,
    outcome: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    failure_reason: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    ip_address: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    user_agent: String,
    attempted_at: DateTime<Utc>,
}

/// `GET /auth/login-history` (Go `handleLoginHistory`).
async fn login_history(State(s): State<Arc<AccountState>>, auth: OptionalAuth) -> Response {
    let p = match principal_from_session(&s, &auth).await {
        Ok(p) => p,
        Err(resp) => return *resp,
    };
    let email = email_of(&p).trim().to_lowercase();
    if email.is_empty() {
        return Json(json!({ "attempts": [] })).into_response();
    }
    let rows = match s
        .two_factor
        .login_attempt_repo
        .find_recent_by_identifier(&email, 20)
        .await
    {
        Ok(rows) => rows,
        Err(_) => return server_error("HISTORY_FAILED", "could not load sign-in history"),
    };
    let attempts: Vec<LoginHistoryItem> = rows
        .into_iter()
        .map(|a| LoginHistoryItem {
            attempt_type: a.attempt_type.as_str().to_string(),
            outcome: a.outcome.as_str().to_string(),
            failure_reason: a.failure_reason.unwrap_or_default(),
            ip_address: a.ip_address.unwrap_or_default(),
            user_agent: a.user_agent.unwrap_or_default(),
            attempted_at: a.attempted_at,
        })
        .collect();
    Json(json!({ "attempts": attempts })).into_response()
}

/// The session-gated account routes, nested under `/auth`.
pub fn account_router(state: Arc<AccountState>) -> Router {
    Router::new()
        .route("/change-password", post(change_password))
        .route("/change-password/send-email-code", post(send_email_code))
        .route("/login-history", get(login_history))
        .with_state(state)
}
