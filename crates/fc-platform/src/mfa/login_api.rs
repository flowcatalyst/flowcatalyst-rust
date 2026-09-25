//! The sign-in half of 2FA (Go `auth/login/twofactor.go`): the decision
//! `/auth/login` makes once the password verifies, and the step-token-gated
//! routes that finish the sign-in:
//!
//! - `POST /auth/2fa/verify` — a code (TOTP, email PIN or recovery code)
//! - `POST /auth/2fa/challenge/email` — send a login PIN
//! - `POST /auth/2fa/enroll/{totp,email}/{begin,confirm}` — enrol when the
//!   domain requires a factor and the user has none
//!
//! The step token stands in for a session, so these routes are public (like
//! `/auth/login`), rate-limited with it, and every failure is the same 401.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, warn};

use super::entity::MethodType;
use super::notify::Notifier;
use super::policy::{Eval, TwoFactorPolicy};
use super::service::{EnrollError, MfaService};
use super::token::{MfaTokenIssuer, Purpose};
use crate::auth::login_backoff::{self, record_user_login_attempt, BackoffDecision, BackoffPolicy};
use crate::auth::session_cookie::SessionCookieConfig;
use crate::shared::middleware::ClientIp;
use crate::shared::rate_limit_store::{
    within_mail_budget, Bucket, RateLimitPolicies, RateLimitStore,
};
use crate::{
    AuditLogRepository, AuthService, LoginAttemptRepository, LoginOutcome, Principal,
    PrincipalRepository, RoleRepository,
};

/// Pending (challenge) token lifetime: 10 minutes.
const PENDING_TOKEN_TTL_SECS: i64 = 10 * 60;
/// Enrolment token lifetime: 30 minutes.
const ENROLL_TOKEN_TTL_SECS: i64 = 30 * 60;
/// The trusted-device cookie: `__Host-` bound when Secure, a bare name over
/// plain-HTTP localhost where the prefix is invalid.
const TRUSTED_DEVICE_COOKIE_PROD: &str = "__Host-fc_td";
const TRUSTED_DEVICE_COOKIE_DEV: &str = "fc_td";

const CODE_SENT: &str = "A verification code has been sent to your email.";

/// Everything the sign-in and self-service 2FA routes (and the
/// password-reset hand-off) share.
pub struct TwoFactorLogin {
    pub mfa: Arc<MfaService>,
    pub tokens: Arc<MfaTokenIssuer>,
    pub policy: TwoFactorPolicy,
    pub notifier: Notifier,
    pub auth_service: Arc<AuthService>,
    pub principal_repo: Arc<PrincipalRepository>,
    pub role_repo: Arc<RoleRepository>,
    pub login_attempt_repo: Arc<LoginAttemptRepository>,
    pub audit_log_repo: Arc<AuditLogRepository>,
    pub backoff_policy: Arc<BackoffPolicy>,
    pub session_cookie: SessionCookieConfig,
    pub rate_limit_store: Arc<dyn RateLimitStore>,
    pub rate_limit_policies: Arc<RateLimitPolicies>,
}

// ── wire shapes ────────────────────────────────────────────────────────────

/// The pending outcomes of `/auth/login` (Go `twoFactorResponse`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TwoFactorResponse {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    mfa_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enroll_token: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    methods: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    allowed_methods: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    remember_device_allowed: bool,
}

/// A completed sign-in (Go `loginResponse`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletedLogin {
    pub status: &'static str,
    pub principal_id: String,
    pub name: String,
    pub email: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
    /// `null` when the user has no home client (Go omits no `omitempty`).
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_codes: Option<Vec<String>>,
    pub sso_managed: bool,
}

// ── response helpers (Go's hand-written JSON bodies) ────────────────────────

pub(crate) fn coded(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({ "code": code, "message": message }))).into_response()
}

/// Go `writeUnauthorized`: 401, the cookie challenge, `UNAUTHENTICATED`.
pub(crate) fn unauthorized(message: &str) -> Response {
    let mut resp = coded(StatusCode::UNAUTHORIZED, "UNAUTHENTICATED", message);
    resp.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Cookie realm=\"fc_session\""),
    );
    resp
}

/// Go `writeTooManyRequests`.
pub(crate) fn too_many_requests(retry_after_secs: u32) -> Response {
    let mut resp = coded(
        StatusCode::TOO_MANY_REQUESTS,
        "TOO_MANY_REQUESTS",
        "too many failed login attempts; try again later",
    );
    resp.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from(u64::from(retry_after_secs)),
    );
    resp
}

/// Go `decodeJSON`: a malformed body is 400 `INVALID_JSON`.
pub(crate) fn decode<T: for<'de> Deserialize<'de>>(body: &Bytes) -> Result<T, Response> {
    serde_json::from_slice(body).map_err(|_| {
        coded(
            StatusCode::BAD_REQUEST,
            "INVALID_JSON",
            "malformed request body",
        )
    })
}

/// Go `writeEnrollErr`.
pub(crate) fn enroll_error(e: EnrollError) -> Response {
    match e {
        EnrollError::AlreadyEnrolled => coded(
            StatusCode::CONFLICT,
            "ALREADY_ENROLLED",
            "that method is already set up",
        ),
        EnrollError::EncryptionUnavailable => coded(
            StatusCode::SERVICE_UNAVAILABLE,
            "TOTP_UNAVAILABLE",
            "authenticator-app 2FA is not available",
        ),
        EnrollError::NoPendingEnrollment => coded(
            StatusCode::BAD_REQUEST,
            "NO_PENDING_ENROLLMENT",
            "start enrollment first",
        ),
        EnrollError::Other(e) => {
            error!(error = %e, "2FA enrollment error");
            coded(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ENROLL_FAILED",
                "could not complete enrollment",
            )
        }
    }
}

pub(crate) fn email_of(p: &Principal) -> String {
    p.email().unwrap_or_default().to_string()
}

fn method_strings(methods: &[MethodType]) -> Vec<String> {
    methods.iter().map(|m| m.as_str().to_string()).collect()
}

/// The browser's User-Agent as a device label, at most 250 bytes.
fn user_agent_label(headers: &HeaderMap) -> Option<String> {
    let ua = headers.get(header::USER_AGENT)?.to_str().ok()?.trim();
    if ua.is_empty() {
        return None;
    }
    let mut end = ua.len().min(250);
    while !ua.is_char_boundary(end) {
        end -= 1;
    }
    Some(ua[..end].to_string())
}

impl TwoFactorLogin {
    fn trusted_device_cookie_name(&self) -> &'static str {
        if self.session_cookie.secure {
            TRUSTED_DEVICE_COOKIE_PROD
        } else {
            TRUSTED_DEVICE_COOKIE_DEV
        }
    }

    /// Whether the account's credentials live with a federated identity
    /// provider (Go `ssoManaged`).
    pub async fn sso_managed(&self, p: &Principal, eval: Option<&Eval>) -> bool {
        if p.external_identity.is_some() {
            return true;
        }
        let owned;
        let eval = match eval {
            Some(e) => e,
            None => {
                owned = self.policy.evaluate(&email_of(p)).await;
                &owned
            }
        };
        eval.mapping.is_some() && !eval.internal
    }

    /// The flattened permissions of the user's roles, with `*` for the
    /// super-admin wildcard (Go `buildPermissionList`).
    async fn permissions(&self, roles: &[String]) -> Vec<String> {
        let mut permissions = self
            .role_repo
            .flatten_permissions(roles)
            .await
            .unwrap_or_default();
        if permissions
            .iter()
            .any(|p| p == crate::role::entity::permissions::ADMIN_ALL)
        {
            permissions.push("*".to_string());
        }
        permissions
    }

    /// The load-bearing half of a sign-in: mint the session cookie, record
    /// the successful attempt, answer Go's login body (Go `completeLogin`).
    /// `recovery_codes` is set only on the enrol-and-complete path, the one
    /// time a fresh set is shown.
    pub async fn complete_login(
        &self,
        jar: CookieJar,
        p: &Principal,
        recovery_codes: Option<Vec<String>>,
        ip: Option<&str>,
    ) -> Response {
        let token = match self.auth_service.generate_session_token(p) {
            Ok(t) => t,
            Err(e) => {
                error!(principal_id = %p.id, error = %e, "session mint failed");
                return coded(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "MINT_FAILED",
                    "failed to mint session token",
                );
            }
        };
        let jar = jar.add(self.session_cookie.build_cookie(token));
        let email = email_of(p);
        record_user_login_attempt(
            &self.login_attempt_repo,
            Some(&email.to_lowercase()),
            Some(&p.id),
            ip,
            LoginOutcome::Success,
            None,
        )
        .await;
        let roles = crate::auth::auth_service::role_names(p);
        let permissions = self.permissions(&roles).await;
        let body = CompletedLogin {
            status: "ok",
            principal_id: p.id.clone(),
            name: p.name.clone(),
            email,
            roles,
            permissions,
            client_id: p.client_id.clone(),
            recovery_codes,
            sso_managed: self.sso_managed(p, None).await,
        };
        (jar, Json(body)).into_response()
    }

    /// Whether this just-authenticated password user owes a second factor.
    /// `Some(response)` (the `mfa_required` / `enrollment_required` body)
    /// when they do; `None` to finish the sign-in. An error means "couldn't
    /// decide", and the caller fails closed (Go `maybeChallenge2FA`).
    pub async fn maybe_challenge(
        &self,
        jar: &CookieJar,
        p: &Principal,
    ) -> crate::shared::error::Result<Option<Response>> {
        // Federated users never carry a password; defensive.
        if p.external_identity.is_some() {
            return Ok(None);
        }
        let eval = self.policy.evaluate(&email_of(p)).await;
        let domain_requires = eval.requires_2fa();
        let remember_allowed = eval
            .mapping
            .as_ref()
            .is_some_and(|m| m.remember_device_enabled);

        let confirmed = self.mfa.confirmed_methods(&p.id).await?;
        let mut usable = method_strings(&confirmed);
        if domain_requires {
            let allowed = eval.allowed_methods();
            usable.retain(|m| allowed.contains(m));
        }

        if !usable.is_empty() {
            if remember_allowed {
                if let Some(c) = jar.get(self.trusted_device_cookie_name()) {
                    if self.mfa.verify_trusted_device(&p.id, c.value()).await? {
                        return Ok(None);
                    }
                }
            }
            let token = self
                .tokens
                .mint(&p.id, Purpose::Pending, PENDING_TOKEN_TTL_SECS)
                .ok_or_else(|| crate::shared::error::PlatformError::internal("mint mfa token"))?;
            return Ok(Some(
                Json(TwoFactorResponse {
                    status: "mfa_required",
                    mfa_token: Some(token),
                    enroll_token: None,
                    methods: usable,
                    allowed_methods: Vec::new(),
                    remember_device_allowed: remember_allowed,
                })
                .into_response(),
            ));
        }

        // No usable factor: only the domain can compel enrolment. A passkey
        // doesn't exempt the password path.
        if !domain_requires {
            return Ok(None);
        }
        let token = self
            .tokens
            .mint(&p.id, Purpose::Enroll, ENROLL_TOKEN_TTL_SECS)
            .ok_or_else(|| crate::shared::error::PlatformError::internal("mint mfa token"))?;
        Ok(Some(
            Json(TwoFactorResponse {
                status: "enrollment_required",
                mfa_token: None,
                enroll_token: Some(token),
                methods: Vec::new(),
                allowed_methods: eval.allowed_methods(),
                remember_device_allowed: false,
            })
            .into_response(),
        ))
    }

    /// Mint an enrolment token for a user who just set a password on a
    /// domain requiring 2FA (the password-reset hand-off).
    pub fn mint_enroll_token(&self, principal_id: &str) -> Option<String> {
        self.tokens
            .mint(principal_id, Purpose::Enroll, ENROLL_TOKEN_TTL_SECS)
    }

    /// The active principal a step token names, or Go's 401.
    async fn principal_from_token(
        &self,
        token: &str,
        purpose: Purpose,
    ) -> Result<Principal, Response> {
        let invalid = || unauthorized("Invalid or expired session");
        let subject = self.tokens.parse(token, purpose).ok_or_else(invalid)?;
        match self.principal_repo.find_by_id(&subject).await {
            Ok(Some(p)) if p.active => Ok(p),
            _ => Err(invalid()),
        }
    }

    /// Generate a first recovery-code set if the user has TOTP and none
    /// left; the codes (shown once) or `None`. Email is its own recovery
    /// channel, so an email-only user never gets them (Go
    /// `ensureRecoveryCodes`).
    pub async fn ensure_recovery_codes(&self, p: &Principal) -> Option<Vec<String>> {
        let confirmed = self.mfa.confirmed_methods(&p.id).await.ok()?;
        if !confirmed.contains(&MethodType::Totp) {
            return None;
        }
        if self.mfa.remaining_recovery_codes(&p.id).await.ok()? > 0 {
            return None;
        }
        match self.mfa.generate_recovery_codes(&p.id).await {
            Ok(codes) => {
                self.notifier.recovery_codes_regenerated(&email_of(p)).await;
                Some(codes)
            }
            Err(e) => {
                error!(principal_id = %p.id, error = %e, "recovery code generation failed");
                None
            }
        }
    }

    /// Remember this browser when the domain allows it: store the device,
    /// set the cookie, tell the user (Go `rememberDevice`).
    async fn remember_device(
        &self,
        jar: CookieJar,
        headers: &HeaderMap,
        p: &Principal,
    ) -> CookieJar {
        let eval = self.policy.evaluate(&email_of(p)).await;
        if !eval.remember_enabled() {
            return jar;
        }
        let days = eval.remember_days();
        let label = user_agent_label(headers);
        let raw = match self
            .mfa
            .issue_trusted_device(&p.id, label.as_deref(), chrono::Duration::days(days))
            .await
        {
            Ok(raw) => raw,
            Err(e) => {
                error!(principal_id = %p.id, error = %e, "issue trusted device failed");
                return jar;
            }
        };
        let cookie = Cookie::build((self.trusted_device_cookie_name(), raw))
            .path("/")
            .http_only(true)
            .secure(self.session_cookie.secure)
            .same_site(SameSite::Strict)
            .max_age(time::Duration::days(days))
            .expires(time::OffsetDateTime::now_utc() + time::Duration::days(days))
            .build();
        self.notifier
            .new_trusted_device(&email_of(p), label.as_deref().unwrap_or(""))
            .await;
        jar.add(cookie)
    }

    /// Forget the browser's remembered-device cookie (the server-side row
    /// is what really revokes it).
    pub fn clear_trusted_device_cookie(&self, jar: CookieJar) -> CookieJar {
        let cookie = Cookie::build((self.trusted_device_cookie_name(), ""))
            .path("/")
            .http_only(true)
            .secure(self.session_cookie.secure)
            .same_site(SameSite::Strict)
            .max_age(time::Duration::seconds(0))
            .build();
        jar.add(cookie)
    }
}

// ── handlers ───────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct VerifyRequest {
    mfa_token: String,
    method: String,
    code: String,
    remember_device: bool,
}

/// `POST /auth/2fa/verify` (Go `handle2FAVerify`).
async fn verify(
    State(s): State<Arc<TwoFactorLogin>>,
    ClientIp(ip): ClientIp,
    jar: CookieJar,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let req: VerifyRequest = match decode(&body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let p = match s
        .principal_from_token(&req.mfa_token, Purpose::Pending)
        .await
    {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let ip = ip.as_deref();
    let email = email_of(&p).to_lowercase();

    // The same (email, IP) backoff as the password step; wrong codes are
    // recorded below, so they throttle like wrong passwords.
    if let Ok(BackoffDecision::Reject {
        retry_after_secs, ..
    }) = login_backoff::check(&s.login_attempt_repo, &s.backoff_policy, &email, ip).await
    {
        return too_many_requests(retry_after_secs);
    }

    let method = req.method.trim().to_uppercase();
    // Owner ruling I-Q12 (Java, 2026-09-05): a domain's allow-list binds the
    // challenge too, not only enrolment. Invisible to a user of an allowed
    // method.
    if matches!(method.as_str(), "TOTP" | "EMAIL_PIN") {
        let eval = s.policy.evaluate(&email).await;
        if !eval.method_allowed(&method) {
            record_user_login_attempt(
                &s.login_attempt_repo,
                Some(&email),
                Some(&p.id),
                ip,
                LoginOutcome::Failure,
                Some("Invalid 2FA code"),
            )
            .await;
            return unauthorized("Invalid or expired code");
        }
    }
    let result = match method.as_str() {
        "TOTP" => s.mfa.verify_totp(&p.id, &req.code).await,
        "EMAIL_PIN" => s.mfa.verify_login_email_pin(&p.id, &req.code).await,
        "RECOVERY_CODE" => s.mfa.verify_recovery_code(&p.id, &req.code).await,
        _ => {
            return coded(
                StatusCode::BAD_REQUEST,
                "INVALID_METHOD",
                "unknown 2FA method",
            )
        }
    };
    let ok = match result {
        Ok(ok) => ok,
        Err(e) => {
            error!(principal_id = %p.id, error = %e, "2FA verify failed");
            return coded(
                StatusCode::INTERNAL_SERVER_ERROR,
                "VERIFY_FAILED",
                "could not verify code",
            );
        }
    };
    if !ok {
        record_user_login_attempt(
            &s.login_attempt_repo,
            Some(&email),
            Some(&p.id),
            ip,
            LoginOutcome::Failure,
            Some("Invalid 2FA code"),
        )
        .await;
        return unauthorized("Invalid or expired code");
    }

    if method == "RECOVERY_CODE" {
        s.notifier.recovery_code_used(&email_of(&p)).await;
    }
    let jar = if req.remember_device {
        s.remember_device(jar, &headers, &p).await
    } else {
        jar
    };
    s.complete_login(jar, &p, None, ip).await
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct TokenOnly {
    mfa_token: String,
    enroll_token: String,
}

/// `POST /auth/2fa/challenge/email` (Go `handle2FAChallengeEmail`). Budgeted
/// per IP and per address (owner ruling 7): over budget nothing is sent and
/// the answer is the same.
async fn challenge_email(
    State(s): State<Arc<TwoFactorLogin>>,
    ClientIp(ip): ClientIp,
    body: Bytes,
) -> Response {
    let req: TokenOnly = match decode(&body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let p = match s
        .principal_from_token(&req.mfa_token, Purpose::Pending)
        .await
    {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let email = email_of(&p);
    if email.is_empty() {
        return coded(StatusCode::BAD_REQUEST, "NO_EMAIL", "account has no email");
    }
    let policies = &s.rate_limit_policies;
    if !within_mail_budget(
        s.rate_limit_store.as_ref(),
        (Bucket::TWO_FACTOR_EMAIL_IP, Bucket::TWO_FACTOR_EMAIL),
        (policies.password_reset_ip, policies.password_reset_email),
        ip.as_deref(),
        &email,
    )
    .await
    {
        warn!(principal_id = %p.id, "2FA email challenge rate limited; no code sent");
        return Json(json!({ "message": CODE_SENT })).into_response();
    }
    if let Err(e) = s.mfa.send_login_email_pin(&p.id, &email).await {
        error!(principal_id = %p.id, error = %e, "send email pin failed");
        return coded(
            StatusCode::BAD_GATEWAY,
            "EMAIL_SEND_FAILED",
            "could not send code",
        );
    }
    Json(json!({ "message": CODE_SENT })).into_response()
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct EnrollConfirmRequest {
    enroll_token: String,
    code: String,
}

/// `POST /auth/2fa/enroll/totp/begin`.
async fn enroll_totp_begin(State(s): State<Arc<TwoFactorLogin>>, body: Bytes) -> Response {
    let req: TokenOnly = match decode(&body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let p = match s
        .principal_from_token(&req.enroll_token, Purpose::Enroll)
        .await
    {
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

/// `POST /auth/2fa/enroll/totp/confirm`: enrols, then finishes the sign-in
/// with a first recovery-code set.
async fn enroll_totp_confirm(
    State(s): State<Arc<TwoFactorLogin>>,
    ClientIp(ip): ClientIp,
    jar: CookieJar,
    body: Bytes,
) -> Response {
    let req: EnrollConfirmRequest = match decode(&body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let p = match s
        .principal_from_token(&req.enroll_token, Purpose::Enroll)
        .await
    {
        Ok(p) => p,
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
    let codes = s.ensure_recovery_codes(&p).await;
    s.complete_login(jar, &p, codes, ip.as_deref()).await
}

/// `POST /auth/2fa/enroll/email/begin`.
async fn enroll_email_begin(State(s): State<Arc<TwoFactorLogin>>, body: Bytes) -> Response {
    let req: TokenOnly = match decode(&body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let p = match s
        .principal_from_token(&req.enroll_token, Purpose::Enroll)
        .await
    {
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
        Ok(()) => Json(json!({ "message": CODE_SENT })).into_response(),
        Err(e) => enroll_error(e),
    }
}

/// `POST /auth/2fa/enroll/email/confirm`.
async fn enroll_email_confirm(
    State(s): State<Arc<TwoFactorLogin>>,
    ClientIp(ip): ClientIp,
    jar: CookieJar,
    body: Bytes,
) -> Response {
    let req: EnrollConfirmRequest = match decode(&body) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let p = match s
        .principal_from_token(&req.enroll_token, Purpose::Enroll)
        .await
    {
        Ok(p) => p,
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
    let codes = s.ensure_recovery_codes(&p).await;
    s.complete_login(jar, &p, codes, ip.as_deref()).await
}

/// The token-gated `/auth/2fa/*` routes, nested under `/auth`.
pub fn two_factor_login_router(state: Arc<TwoFactorLogin>) -> Router {
    Router::new()
        .route("/2fa/verify", post(verify))
        .route("/2fa/challenge/email", post(challenge_email))
        .route("/2fa/enroll/totp/begin", post(enroll_totp_begin))
        .route("/2fa/enroll/totp/confirm", post(enroll_totp_confirm))
        .route("/2fa/enroll/email/begin", post(enroll_email_begin))
        .route("/2fa/enroll/email/confirm", post(enroll_email_confirm))
        .with_state(state)
}
