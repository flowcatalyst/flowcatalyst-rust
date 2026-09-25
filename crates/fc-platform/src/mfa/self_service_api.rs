//! A signed-in user's own 2FA (Go `auth/login/twofactor_selfservice.go`),
//! for the Profile screen:
//!
//! - `GET /auth/2fa/status`
//! - `POST /auth/2fa/methods/{totp,email}/{begin,confirm}`
//! - `DELETE /auth/2fa/methods/{method}`
//! - `POST /auth/2fa/recovery-codes/regenerate`
//! - `GET /auth/2fa/trusted-devices`, `DELETE /auth/2fa/trusted-devices/{id}`
//!
//! Every route acts on the caller's own principal, reloaded and required
//! active, as Go's `principalFromSession`; there is no one else's 2FA to
//! reach here, so authentication is the gate.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::entity::MethodType;
use super::login_api::{coded, decode, email_of, enroll_error, unauthorized, TwoFactorLogin};
use crate::shared::middleware::Authenticated;
use crate::Principal;

/// The menu when a domain doesn't restrict methods.
const ALL_METHODS: [&str; 2] = ["TOTP", "EMAIL_PIN"];

async fn principal_from_session(
    s: &TwoFactorLogin,
    auth: &Authenticated,
) -> Result<Principal, Response> {
    match s.principal_repo.find_by_id(&auth.0.principal_id).await {
        Ok(Some(p)) if p.active => Ok(p),
        _ => Err(unauthorized("Not authenticated")),
    }
}

fn server_error(code: &str, message: &str) -> Response {
    coded(StatusCode::INTERNAL_SERVER_ERROR, code, message)
}

fn recovery_codes_body(codes: Option<Vec<String>>) -> Response {
    Json(json!({ "recoveryCodes": codes.unwrap_or_default() })).into_response()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    methods: Vec<String>,
    required: bool,
    allowed_methods: Vec<String>,
    recovery_codes_left: i64,
    remember_device_enabled: bool,
    trusted_device_count: usize,
}

/// `GET /auth/2fa/status`.
async fn status(State(s): State<Arc<TwoFactorLogin>>, auth: Authenticated) -> Response {
    let p = match principal_from_session(&s, &auth).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let confirmed = match s.mfa.confirmed_methods(&p.id).await {
        Ok(c) => c,
        Err(_) => return server_error("STATUS_FAILED", "could not load 2FA status"),
    };
    let eval = s.policy.evaluate(&email_of(&p)).await;
    let required = eval.requires_2fa();
    let allowed_methods = if required {
        eval.allowed_methods()
    } else {
        ALL_METHODS.iter().map(|m| m.to_string()).collect()
    };
    let recovery_codes_left = s.mfa.remaining_recovery_codes(&p.id).await.unwrap_or(0);
    let trusted_device_count = s
        .mfa
        .list_trusted_devices(&p.id)
        .await
        .map(|d| d.len())
        .unwrap_or(0);
    Json(StatusResponse {
        methods: confirmed.iter().map(|m| m.as_str().to_string()).collect(),
        required,
        allowed_methods,
        recovery_codes_left,
        remember_device_enabled: eval.remember_enabled(),
        trusted_device_count,
    })
    .into_response()
}

/// `POST /auth/2fa/methods/totp/begin`.
async fn totp_begin(State(s): State<Arc<TwoFactorLogin>>, auth: Authenticated) -> Response {
    let p = match principal_from_session(&s, &auth).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    if !s
        .policy
        .evaluate(&email_of(&p))
        .await
        .method_allowed("TOTP")
    {
        return coded(
            StatusCode::FORBIDDEN,
            "METHOD_NOT_ALLOWED",
            "authenticator app is not permitted for this domain",
        );
    }
    match s.mfa.begin_totp_enrollment(&p.id, &email_of(&p)).await {
        Ok(e) => Json(json!({ "secret": e.secret, "uri": e.uri, "qr": e.qr })).into_response(),
        Err(e) => enroll_error(e),
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CodeRequest {
    code: String,
}

/// `POST /auth/2fa/methods/totp/confirm`: the first recovery-code set
/// comes back (empty when the user already had codes).
async fn totp_confirm(
    State(s): State<Arc<TwoFactorLogin>>,
    auth: Authenticated,
    body: Bytes,
) -> Response {
    let p = match principal_from_session(&s, &auth).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let req: CodeRequest = match decode(&body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    match s.mfa.confirm_totp_enrollment(&p.id, &req.code).await {
        Ok(true) => {}
        Ok(false) => {
            return coded(
                StatusCode::BAD_REQUEST,
                "INVALID_CODE",
                "that code didn't match — try again",
            )
        }
        Err(e) => return enroll_error(e),
    }
    s.notifier.two_factor_enrolled(&email_of(&p), "TOTP").await;
    super::audit::record(&s.audit_log_repo, &p.id, super::audit::TOTP_ENROLLED, &p.id).await;
    recovery_codes_body(s.ensure_recovery_codes(&p).await)
}

/// `POST /auth/2fa/methods/email/begin`.
async fn email_begin(State(s): State<Arc<TwoFactorLogin>>, auth: Authenticated) -> Response {
    let p = match principal_from_session(&s, &auth).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    if !s
        .policy
        .evaluate(&email_of(&p))
        .await
        .method_allowed("EMAIL_PIN")
    {
        return coded(
            StatusCode::FORBIDDEN,
            "METHOD_NOT_ALLOWED",
            "email codes are not permitted for this domain",
        );
    }
    let email = email_of(&p);
    if email.is_empty() {
        return coded(StatusCode::BAD_REQUEST, "NO_EMAIL", "account has no email");
    }
    match s.mfa.begin_email_enrollment(&p.id, &email).await {
        Ok(()) => Json(json!({
            "message": "A verification code has been sent to your email."
        }))
        .into_response(),
        Err(e) => enroll_error(e),
    }
}

/// `POST /auth/2fa/methods/email/confirm`.
async fn email_confirm(
    State(s): State<Arc<TwoFactorLogin>>,
    auth: Authenticated,
    body: Bytes,
) -> Response {
    let p = match principal_from_session(&s, &auth).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let req: CodeRequest = match decode(&body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    match s.mfa.confirm_email_enrollment(&p.id, &req.code).await {
        Ok(true) => {}
        Ok(false) => {
            return coded(
                StatusCode::BAD_REQUEST,
                "INVALID_CODE",
                "that code didn't match — try again",
            )
        }
        Err(e) => return enroll_error(e),
    }
    s.notifier
        .two_factor_enrolled(&email_of(&p), "EMAIL_PIN")
        .await;
    super::audit::record(
        &s.audit_log_repo,
        &p.id,
        super::audit::EMAIL_ENROLLED,
        &p.id,
    )
    .await;
    recovery_codes_body(s.ensure_recovery_codes(&p).await)
}

/// `DELETE /auth/2fa/methods/{method}`. A user whose domain requires 2FA
/// can't remove their last confirmed factor.
async fn remove_method(
    State(s): State<Arc<TwoFactorLogin>>,
    auth: Authenticated,
    Path(method): Path<String>,
) -> Response {
    let p = match principal_from_session(&s, &auth).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let Ok(method_type) = method.parse::<MethodType>() else {
        return coded(
            StatusCode::BAD_REQUEST,
            "INVALID_METHOD",
            "unknown 2FA method",
        );
    };
    let confirmed = match s.mfa.confirmed_methods(&p.id).await {
        Ok(c) => c,
        Err(_) => return server_error("REMOVE_FAILED", "could not load methods"),
    };
    if s.policy.evaluate(&email_of(&p)).await.requires_2fa()
        && confirmed.iter().all(|m| *m == method_type)
    {
        return coded(
            StatusCode::CONFLICT,
            "LAST_FACTOR",
            "your organisation requires 2FA — add another method before removing this one",
        );
    }
    if s.mfa.remove_method(&p.id, method_type).await.is_err() {
        return server_error("REMOVE_FAILED", "could not remove method");
    }
    s.notifier
        .two_factor_method_removed(&email_of(&p), method_type.as_str())
        .await;
    super::audit::record(
        &s.audit_log_repo,
        &p.id,
        super::audit::METHOD_REMOVED,
        &p.id,
    )
    .await;
    Json(json!({ "message": "Two-factor method removed." })).into_response()
}

/// `POST /auth/2fa/recovery-codes/regenerate`: only for authenticator-app
/// users.
async fn regenerate_recovery_codes(
    State(s): State<Arc<TwoFactorLogin>>,
    auth: Authenticated,
) -> Response {
    let p = match principal_from_session(&s, &auth).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let confirmed = match s.mfa.confirmed_methods(&p.id).await {
        Ok(c) => c,
        Err(_) => return server_error("REGEN_FAILED", "could not load methods"),
    };
    if !confirmed.contains(&MethodType::Totp) {
        return coded(
            StatusCode::BAD_REQUEST,
            "NO_TOTP",
            "recovery codes apply to authenticator-app 2FA",
        );
    }
    let codes = match s.mfa.generate_recovery_codes(&p.id).await {
        Ok(c) => c,
        Err(_) => return server_error("REGEN_FAILED", "could not generate recovery codes"),
    };
    s.notifier.recovery_codes_regenerated(&email_of(&p)).await;
    super::audit::record(
        &s.audit_log_repo,
        &p.id,
        super::audit::RECOVERY_REGENERATED,
        &p.id,
    )
    .await;
    recovery_codes_body(Some(codes))
}

/// `GET /auth/2fa/trusted-devices`.
async fn list_trusted_devices(
    State(s): State<Arc<TwoFactorLogin>>,
    auth: Authenticated,
) -> Response {
    let p = match principal_from_session(&s, &auth).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    match s.mfa.list_trusted_devices(&p.id).await {
        Ok(devices) => Json(json!({ "devices": devices })).into_response(),
        Err(_) => server_error("LIST_FAILED", "could not list devices"),
    }
}

/// `DELETE /auth/2fa/trusted-devices/{id}` (only the caller's own).
async fn revoke_trusted_device(
    State(s): State<Arc<TwoFactorLogin>>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Response {
    let p = match principal_from_session(&s, &auth).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    if s.mfa.revoke_trusted_device(&p.id, &id).await.is_err() {
        return server_error("REVOKE_FAILED", "could not revoke device");
    }
    Json(json!({ "message": "Device removed." })).into_response()
}

/// The session-gated `/auth/2fa/*` routes, nested under `/auth`.
pub fn two_factor_self_service_router(state: Arc<TwoFactorLogin>) -> Router {
    Router::new()
        .route("/2fa/status", get(status))
        .route("/2fa/methods/totp/begin", post(totp_begin))
        .route("/2fa/methods/totp/confirm", post(totp_confirm))
        .route("/2fa/methods/email/begin", post(email_begin))
        .route("/2fa/methods/email/confirm", post(email_confirm))
        .route("/2fa/methods/{method}", delete(remove_method))
        .route(
            "/2fa/recovery-codes/regenerate",
            post(regenerate_recovery_codes),
        )
        .route("/2fa/trusted-devices", get(list_trusted_devices))
        .route("/2fa/trusted-devices/{id}", delete(revoke_trusted_device))
        .with_state(state)
}
