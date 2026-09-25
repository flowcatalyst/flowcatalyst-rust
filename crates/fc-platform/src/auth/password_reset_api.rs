//! `/auth/password-reset/*` and `/auth/password-setup/request` — the
//! unauthenticated password flows (Go `passwordreset/api/api.go`):
//! request → (email link) → validate → confirm, for a forgotten password
//! (a `reset` token, 15 minutes) and a first-time "set your password"
//! invite (an `invite` token, 72 hours). Tokens are stored as SHA-256
//! hashes; the raw token is only ever in the emailed link.

use axum::{
    body::Bytes,
    extract::{Query, State},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use axum_extra::extract::cookie::CookieJar;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tracing::{info, warn};
use utoipa::ToSchema;

use crate::auth::password_service::PasswordService;
use crate::mfa::entity::MethodType;
use crate::mfa::TwoFactorLogin;
use crate::password_reset::entity::{PasswordResetToken, TokenPurpose};
use crate::password_reset::repository::PasswordResetTokenRepository;
use crate::principal::entity::Principal;
use crate::principal::operations::events::PasswordResetRequested;
use crate::principal::repository::PrincipalRepository;
use crate::shared::email_service::{EmailMessage, EmailService};
use crate::shared::error::PlatformError;
use crate::shared::middleware::ClientIp;
use crate::shared::rate_limit_store::{
    within_mail_budget, Bucket, RateLimitPolicies, RateLimitStore,
};
use crate::{PgUnitOfWork, UnitOfWork};

/// A reset token's lifetime (15 minutes).
const RESET_TOKEN_TTL_MINUTES: i64 = 15;
/// An invite's lifetime (72 hours): time for a new user to act on it.
const INVITE_TOKEN_TTL_HOURS: i64 = 72;
/// Wrong authenticator codes against a factor-gated reset token before the
/// principal's whole token set is burned (Go `maxFactorAttempts`).
const MAX_FACTOR_ATTEMPTS: i32 = 5;

/// Mints single-use tokens and emails the links. Used by the self-service
/// request routes and by the admin `send-password-reset` and create-user
/// paths.
#[derive(Clone)]
pub struct PasswordResetEmailer {
    pub password_reset_repo: Arc<PasswordResetTokenRepository>,
    pub email_service: Arc<dyn EmailService>,
    pub unit_of_work: Arc<PgUnitOfWork>,
    /// Base URL for the links (e.g. "https://app.flowcatalyst.io")
    pub external_base_url: String,
}

/// What a reset token carries beyond its principal.
#[derive(Debug, Clone, Default)]
pub struct ResetOptions {
    /// The confirm also clears the user's second factors.
    pub reset_2fa: bool,
    /// The confirm also needs a current authenticator code.
    pub requires_factor: bool,
    /// Where the SPA resumes once the reset completes.
    pub redirect_uri: Option<String>,
}

impl PasswordResetEmailer {
    /// A fresh single-use reset token (15 minutes) for `principal`, and the
    /// emailed link. Email failures are logged, not returned (the token is
    /// still valid). The caller has checked the principal is eligible.
    pub async fn send_reset_email(&self, principal: &Principal) -> Result<(), PlatformError> {
        self.send_reset_email_with(principal, ResetOptions::default())
            .await
    }

    /// [`send_reset_email`](Self::send_reset_email) with the token's flags
    /// (Go `SendResetEmail(ctx, p, reset2FA)` and `tryIssueToken`).
    pub async fn send_reset_email_with(
        &self,
        principal: &Principal,
        options: ResetOptions,
    ) -> Result<(), PlatformError> {
        let email = principal
            .user_identity
            .as_ref()
            .map(|i| i.email.clone())
            .ok_or_else(|| {
                PlatformError::validation(
                    "Principal does not have an email address for password reset",
                )
            })?;

        let raw_token = self
            .mint(
                &principal.id,
                TokenPurpose::Reset,
                Utc::now() + Duration::minutes(RESET_TOKEN_TTL_MINUTES),
                options,
            )
            .await?;

        // The SPA's `/auth/reset-password` route (frontend/src/router/index.ts).
        let reset_link = format!(
            "{}/auth/reset-password?token={}",
            self.external_base_url.trim_end_matches('/'),
            raw_token
        );
        let message = EmailMessage {
            to: email.clone(),
            subject: "Reset your password".to_string(),
            html_body: format!(
                "<p>We received a request to reset your password. Click the link below to choose a new one.</p>\
                 <p><a href=\"{}\">Reset password</a></p>\
                 <p>This link expires in 15 minutes.</p>\
                 <p>If you didn't request this, you can safely ignore this email.</p>",
                reset_link
            ),
            text_body: Some(format!(
                "We received a request to reset your password.\n\nReset link: {}\n\nThis link expires in 15 minutes.",
                reset_link
            )),
        };
        if let Err(e) = self.email_service.send(&message).await {
            warn!(principal_id = %principal.id, error = %e, "Failed to send password reset email");
        }

        // Best-effort domain event.
        let event = PasswordResetRequested::new(&principal.id, &email);
        let command = serde_json::json!({ "principalId": principal.id, "email": email });
        if let Err(e) = self
            .unit_of_work
            .emit_event(event, &command)
            .await
            .into_result()
        {
            warn!("Failed to emit PasswordResetRequested event: {}", e);
        }

        Ok(())
    }

    /// A first-time "set your password" invite (72 hours) and its email (Go
    /// `SendInviteRedirect`): the same confirm page, "set" framing, and
    /// `redirect_uri` is followed once the flow completes.
    pub async fn send_invite(
        &self,
        principal: &Principal,
        redirect_uri: Option<String>,
    ) -> Result<(), PlatformError> {
        let Some(email) = principal
            .user_identity
            .as_ref()
            .map(|i| i.email.trim().to_string())
            .filter(|e| !e.is_empty())
        else {
            return Ok(());
        };
        let raw_token = self
            .mint(
                &principal.id,
                TokenPurpose::Invite,
                Utc::now() + Duration::hours(INVITE_TOKEN_TTL_HOURS),
                ResetOptions {
                    redirect_uri,
                    ..ResetOptions::default()
                },
            )
            .await?;
        let link = format!(
            "{}/auth/set-password?token={}",
            self.external_base_url.trim_end_matches('/'),
            raw_token
        );
        let message = EmailMessage {
            to: email,
            subject: "Set your password".to_string(),
            html_body: format!(
                "<p>An account has been created for you. Click the link below to set your password and sign in.</p>\
                 <p><a href=\"{link}\">Set your password</a></p>\
                 <p>If two-factor authentication is required for your organisation, you'll be guided through setting it up.</p>\
                 <p>This link expires in 72 hours.</p>"
            ),
            text_body: Some(format!(
                "An account has been created for you.\n\nSet your password: {link}\n\nThis link expires in 72 hours."
            )),
        };
        self.email_service
            .send(&message)
            .await
            .map_err(|e| PlatformError::internal(format!("send invite email: {e}")))
    }

    /// Mint the same 72-hour invite as [`send_invite`](Self::send_invite)
    /// but return the set-password link instead of emailing it (Go
    /// `InviteLink`, which backs create-user's `returnInviteLink`). The link
    /// is a live bearer credential: hand it only to the authorised caller,
    /// never log it. `None` for a principal without an email.
    pub async fn invite_link(
        &self,
        principal: &Principal,
        redirect_uri: Option<String>,
    ) -> Result<Option<String>, PlatformError> {
        if principal
            .user_identity
            .as_ref()
            .is_none_or(|i| i.email.trim().is_empty())
        {
            return Ok(None);
        }
        let raw_token = self
            .mint(
                &principal.id,
                TokenPurpose::Invite,
                Utc::now() + Duration::hours(INVITE_TOKEN_TTL_HOURS),
                ResetOptions {
                    redirect_uri,
                    ..ResetOptions::default()
                },
            )
            .await?;
        Ok(Some(format!(
            "{}/auth/set-password?token={}",
            self.external_base_url.trim_end_matches('/'),
            raw_token
        )))
    }

    /// Replace the principal's outstanding tokens with a fresh one; the raw
    /// token.
    async fn mint(
        &self,
        principal_id: &str,
        purpose: TokenPurpose,
        expires_at: chrono::DateTime<Utc>,
        options: ResetOptions,
    ) -> Result<String, PlatformError> {
        self.password_reset_repo
            .delete_by_principal_id(principal_id)
            .await?;
        let raw_token = generate_raw_token();
        let mut token = PasswordResetToken::new(principal_id, hash_token(&raw_token), expires_at);
        token.purpose = purpose;
        token.reset_2fa = options.reset_2fa;
        token.requires_factor = options.requires_factor;
        token.redirect_uri = options.redirect_uri;
        self.password_reset_repo.create(&token).await?;
        Ok(raw_token)
    }
}

#[derive(Clone)]
pub struct PasswordResetApiState {
    pub principal_repo: Arc<PrincipalRepository>,
    pub password_service: Arc<PasswordService>,
    pub unit_of_work: Arc<PgUnitOfWork>,
    pub emailer: Arc<PasswordResetEmailer>,
    /// Direct repo access for the validate/confirm endpoints which look up by token.
    pub password_reset_repo: Arc<PasswordResetTokenRepository>,
    /// Use case used by `confirm_reset` so the principal write + event +
    /// audit log are committed atomically.
    pub reset_password_use_case:
        Arc<crate::principal::operations::ResetPasswordUseCase<PgUnitOfWork>>,
    /// The 2FA hand-off (factor-gated resets, `reset_2fa` tokens, the
    /// enrolment gate, the invite's session). None: resets ignore 2FA.
    pub two_factor: Option<Arc<TwoFactorLogin>>,
    /// Refresh tokens minted under the old password die with it.
    pub refresh_token_repo: Arc<crate::RefreshTokenRepository>,
    /// The password-setup request's per-IP and per-address budget.
    pub rate_limit_store: Arc<dyn RateLimitStore>,
    pub rate_limit_policies: Arc<RateLimitPolicies>,
}

// -- Request / Response DTOs --

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestResetBody {
    pub email: String,
    /// The OAuth authorize round-trip to resume after the reset; only a
    /// same-origin `/oauth/authorize?…` is honoured.
    #[serde(default)]
    pub redirect_uri: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ValidateTokenQuery {
    #[serde(default)]
    pub token: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidateTokenResponse {
    pub valid: bool,
    pub reason: Option<String>,
    /// The confirm will also need an authenticator code.
    pub requires_factor: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmResetBody {
    pub token: String,
    pub password: String,
    /// An authenticator (TOTP) code, for a factor-gated token.
    #[serde(default)]
    pub factor_code: String,
}

/// The confirm answer (Go `confirmResponse`): `ok`, or
/// `enrollment_required` with a step token when the domain requires 2FA and
/// the user has no factor.
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmResponse {
    pub status: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enroll_token: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_methods: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uri: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub session_established: bool,
}

// -- Helpers --

/// Hash a raw token to produce the stored token_hash (SHA-256 hex).
fn hash_token(raw_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_token.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Generate a secure random token (URL-safe base64, 32 bytes).
fn generate_raw_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

/// `u` when it is a same-site relative path: one leading `/`, not `//` or
/// `/\` (which browsers read as host-relative). Go `safeRelativeReturnURL`.
fn safe_relative_return_url(u: &str) -> Option<String> {
    (u.starts_with('/') && !u.starts_with("//") && !u.starts_with("/\\")).then(|| u.to_string())
}

/// A self-service reset may only resume the OAuth authorize round-trip the
/// user was in (Go `resetReturnURL`).
fn reset_return_url(u: Option<&str>) -> Option<String> {
    let safe = safe_relative_return_url(u?.trim())?;
    (safe.starts_with("/oauth/authorize?") && safe.len() <= 4096).then_some(safe)
}

/// Why a self-service reset can't be issued for `p` (Go
/// `ineligibleForReset`), or `None`.
fn ineligible_for_reset(p: &Principal) -> Option<&'static str> {
    if !p.is_user() {
        Some("not a USER principal")
    } else if p.external_identity.is_some() {
        Some("OIDC-federated (signs in through an external identity provider; has no platform password)")
    } else if p
        .user_identity
        .as_ref()
        .is_none_or(|i| i.email.trim().is_empty())
    {
        Some("no email address on the account")
    } else {
        None
    }
}

/// An internal (password) USER who has never set a password: active, a
/// user identity with no hash, no linked external identity, not
/// provisioned through OIDC (Go `passwordSetupEligible`; the same rule as
/// `/auth/check-domain`'s `passwordSetupRequired`).
pub fn password_setup_eligible(p: &Principal) -> bool {
    if !p.active || !p.is_user() || p.external_identity.is_some() {
        return false;
    }
    let Some(identity) = p.user_identity.as_ref() else {
        return false;
    };
    identity.password_hash.is_none() && identity.provider.as_deref() != Some("OIDC")
}

fn domain_of(email: &str) -> &str {
    email.rsplit_once('@').map(|(_, d)| d).unwrap_or("")
}

// -- Handlers --

/// Request a password reset email
#[utoipa::path(
    post,
    path = "/request",
    tag = "password-reset",
    operation_id = "postAuthPasswordResetRequest",
    request_body = RequestResetBody,
    responses(
        (status = 200, description = "Reset requested (silent success)", body = MessageResponse)
    )
)]
async fn request_reset(
    State(state): State<PasswordResetApiState>,
    Json(body): Json<RequestResetBody>,
) -> Json<MessageResponse> {
    // Silent success: the same answer whether or not the address exists or
    // was eligible, so the route can't enumerate accounts.
    if let Err(e) = try_issue_reset(&state, &body).await {
        warn!("Password reset request error (suppressed): {}", e);
    }
    Json(MessageResponse {
        message: "If an account exists, a reset email has been sent.".to_string(),
    })
}

/// Go `tryIssueToken`: an eligible user gets a 15-minute link; a user with
/// an authenticator (TOTP) must also prove it at confirm.
async fn try_issue_reset(
    state: &PasswordResetApiState,
    body: &RequestResetBody,
) -> Result<(), PlatformError> {
    let email = body.email.trim().to_lowercase();
    if email.is_empty() {
        return Ok(());
    }
    let Some(principal) = state.principal_repo.find_by_email(&email).await? else {
        // The domain only: the address is PII a caller chose to submit.
        warn!(domain = %domain_of(&email), "Password reset requested for unknown email");
        return Ok(());
    };
    if let Some(reason) = ineligible_for_reset(&principal) {
        info!(principal_id = %principal.id, reason, "password reset requested for an ineligible account; no email sent");
        return Ok(());
    }
    let requires_factor = match &state.two_factor {
        Some(tf) => tf
            .mfa
            .confirmed_methods(&principal.id)
            .await
            .map(|m| m.contains(&MethodType::Totp))
            .unwrap_or(false),
        None => false,
    };
    state
        .emailer
        .send_reset_email_with(
            &principal,
            ResetOptions {
                reset_2fa: false,
                requires_factor,
                redirect_uri: reset_return_url(body.redirect_uri.as_deref()),
            },
        )
        .await
}

/// Validate a password reset token
#[utoipa::path(
    get,
    path = "/validate",
    tag = "password-reset",
    operation_id = "getAuthPasswordResetValidate",
    params(
        ("token" = String, Query, description = "Reset token to validate")
    ),
    responses(
        (status = 200, description = "Token validation result", body = ValidateTokenResponse)
    )
)]
async fn validate_token(
    State(state): State<PasswordResetApiState>,
    Query(query): Query<ValidateTokenQuery>,
) -> Json<ValidateTokenResponse> {
    let invalid = |reason: &str| ValidateTokenResponse {
        valid: false,
        reason: Some(reason.to_string()),
        requires_factor: false,
    };
    match state
        .password_reset_repo
        .find_by_token_hash(&hash_token(&query.token))
        .await
    {
        Ok(Some(token)) if token.is_expired() => Json(invalid("expired")),
        Ok(Some(token)) => Json(ValidateTokenResponse {
            valid: true,
            reason: None,
            requires_factor: token.requires_factor,
        }),
        Ok(None) => Json(invalid("not_found")),
        Err(e) => {
            warn!("Token validation error: {}", e);
            Json(invalid("not_found"))
        }
    }
}

/// Confirm a password reset (consume token and set new password)
#[utoipa::path(
    post,
    path = "/confirm",
    tag = "password-reset",
    operation_id = "postAuthPasswordResetConfirm",
    request_body = ConfirmResetBody,
    responses(
        (status = 200, description = "Password reset successfully"),
        (status = 400, description = "Invalid or expired token")
    )
)]
async fn confirm_reset(
    State(state): State<PasswordResetApiState>,
    jar: CookieJar,
    Json(body): Json<ConfirmResetBody>,
) -> Result<Response, PlatformError> {
    use crate::principal::operations::ResetPasswordCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    let invalid_token =
        || PlatformError::bad_request_code("INVALID_TOKEN", "Invalid or expired reset token.");
    let reset_token = state
        .password_reset_repo
        .find_by_token_hash(&hash_token(&body.token))
        .await?
        .ok_or_else(invalid_token)?;

    if reset_token.is_expired() {
        let _ = state
            .password_reset_repo
            .delete_by_principal_id(&reset_token.principal_id)
            .await;
        return Err(PlatformError::bad_request_code(
            "EXPIRED_TOKEN",
            "Reset token has expired.",
        ));
    }

    // A factor-gated token also needs a current authenticator code. The
    // token survives a wrong code (the user retries) up to the ceiling,
    // then the principal's whole token set is burned. Email PINs never
    // count here.
    if reset_token.requires_factor {
        let Some(tf) = &state.two_factor else {
            return Err(PlatformError::bad_request_code(
                "FACTOR_REQUIRED",
                "Two-factor verification is required.",
            ));
        };
        if reset_token.factor_attempts >= MAX_FACTOR_ATTEMPTS {
            let _ = state
                .password_reset_repo
                .delete_by_principal_id(&reset_token.principal_id)
                .await;
            return Err(invalid_token());
        }
        let ok = tf
            .mfa
            .verify_totp(&reset_token.principal_id, body.factor_code.trim())
            .await?;
        if !ok {
            let n = state
                .password_reset_repo
                .increment_factor_attempts(&reset_token.id)
                .await
                .unwrap_or_else(|e| {
                    warn!(token_id = %reset_token.id, error = %e, "factor attempt increment failed");
                    None
                });
            if n.is_some_and(|n| n >= MAX_FACTOR_ATTEMPTS) {
                warn!(principal_id = %reset_token.principal_id, "password-reset factor attempts exhausted; burning token set");
                let _ = state
                    .password_reset_repo
                    .delete_by_principal_id(&reset_token.principal_id)
                    .await;
                return Err(invalid_token());
            }
            return Err(PlatformError::bad_request_code(
                "INVALID_FACTOR",
                "Invalid authenticator code.",
            ));
        }
    }

    // The password write, its event and audit row commit together through
    // ResetPasswordUseCase; the unauthenticated reset is "system".
    let command = ResetPasswordCommand {
        principal_id: reset_token.principal_id.clone(),
        new_password: body.password,
        enforce_password_complexity: Some(true),
    };
    state
        .reset_password_use_case
        .run(command, ExecutionContext::create("system"))
        .await
        .into_result()?;

    // Single use across the whole set. Best-effort: the password is changed.
    if let Err(e) = state
        .password_reset_repo
        .delete_by_principal_id(&reset_token.principal_id)
        .await
    {
        warn!(principal_id = %reset_token.principal_id, error = %e, "failed to clear consumed reset tokens");
    }
    info!(principal_id = %reset_token.principal_id, "Password reset completed successfully");

    let mut response = post_reset(&state, &reset_token).await;
    response.redirect_uri = reset_token.redirect_uri.clone();

    // Create-your-password: a completed invite that finished plain "ok"
    // signs the user in, unless their domain requires 2FA (minting here
    // would skip the challenge).
    let mut jar = jar;
    if reset_token.purpose == TokenPurpose::Invite && response.status == "ok" {
        if let Some(tf) = &state.two_factor {
            if let Ok(Some(p)) = state
                .principal_repo
                .find_by_id(&reset_token.principal_id)
                .await
            {
                let requires = tf
                    .policy
                    .evaluate(p.email().unwrap_or_default())
                    .await
                    .requires_2fa();
                if !requires {
                    match tf.auth_service.generate_session_token(&p) {
                        Ok(token) => {
                            jar = jar.add(tf.session_cookie.build_cookie(token));
                            response.session_established = true;
                        }
                        Err(e) => {
                            warn!(principal_id = %p.id, error = %e, "post-invite session mint failed")
                        }
                    }
                }
            }
        }
    }
    Ok((jar, Json(response)).into_response())
}

/// After a reset (Go `postResetTwoFactor`): clear 2FA on a `reset_2fa`
/// token, forget remembered devices, revoke refresh tokens, tell the user;
/// then hand an unenrolled user of a 2FA-requiring domain an enrolment
/// token.
async fn post_reset(state: &PasswordResetApiState, token: &PasswordResetToken) -> ConfirmResponse {
    let ok = ConfirmResponse {
        status: "ok",
        message: "Password reset successfully.".to_string(),
        ..ConfirmResponse::default()
    };
    let Ok(Some(p)) = state.principal_repo.find_by_id(&token.principal_id).await else {
        return ok;
    };
    let email = p.email().unwrap_or_default().to_string();

    if let Some(tf) = &state.two_factor {
        if token.reset_2fa {
            match tf.mfa.reset_all(&p.id).await {
                Ok(()) => tf.notifier.two_factor_reset(&email).await,
                Err(e) => {
                    warn!(principal_id = %p.id, error = %e, "2FA reset during password reset failed")
                }
            }
        }
        if let Err(e) = tf.mfa.revoke_all_trusted_devices(&p.id).await {
            warn!(principal_id = %p.id, error = %e, "revoke trusted devices failed");
        }
    }
    if let Err(e) = state
        .refresh_token_repo
        .revoke_all_for_principal(&p.id)
        .await
    {
        warn!(principal_id = %p.id, error = %e, "revoke refresh tokens after reset failed");
    }
    let Some(tf) = &state.two_factor else {
        return ok;
    };
    tf.notifier.password_changed(&email).await;

    let eval = tf.policy.evaluate(&email).await;
    if !eval.requires_2fa() {
        return ok;
    }
    match tf.mfa.has_confirmed_method(&p.id).await {
        Ok(false) => {}
        Ok(true) => return ok,
        Err(e) => {
            warn!(principal_id = %p.id, error = %e, "2FA enrollment check failed");
            return ok;
        }
    }
    match tf.mint_enroll_token(&p.id) {
        Some(enroll_token) => ConfirmResponse {
            status: "enrollment_required",
            message: "Password set. Set up two-factor authentication to finish.".to_string(),
            enroll_token: Some(enroll_token),
            allowed_methods: eval.allowed_methods(),
            ..ConfirmResponse::default()
        },
        None => ok,
    }
}

pub fn password_reset_router(state: PasswordResetApiState) -> Router {
    Router::new()
        .route("/request", post(request_reset))
        .route("/validate", get(validate_token))
        .route("/confirm", post(confirm_reset))
        .with_state(state)
}

// -- /auth/password-setup/request --

const SETUP_REQUESTED: &str =
    "If your account needs a password, we've emailed you a link to create it.";

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct PasswordSetupBody {
    email: String,
    redirect_uri: Option<String>,
}

/// `POST /auth/password-setup/request` (Go `requestPasswordSetup`): emails
/// a "create your password" invite to an internal user who has never set
/// one — the login page offers it when `/auth/check-domain` reports
/// `passwordSetupRequired`. Silent success, like the reset request. Spends
/// the reset budgets per IP and per address in its own buckets (Java
/// dbe3ad9c): over budget nothing is sent and the answer is the same.
async fn request_password_setup(
    State(state): State<PasswordResetApiState>,
    ClientIp(ip): ClientIp,
    body: Bytes,
) -> Response {
    let Ok(body) = serde_json::from_slice::<PasswordSetupBody>(&body) else {
        return PlatformError::bad_request_code("INVALID_BODY", "malformed request body")
            .into_response();
    };
    let email = body.email.trim().to_lowercase();
    let policies = &state.rate_limit_policies;
    if within_mail_budget(
        state.rate_limit_store.as_ref(),
        (Bucket::PASSWORD_SETUP_IP, Bucket::PASSWORD_SETUP_EMAIL),
        (policies.password_reset_ip, policies.password_reset_email),
        ip.as_deref(),
        &email,
    )
    .await
    {
        if let Err(e) = try_issue_password_setup(&state, &email, body.redirect_uri.as_deref()).await
        {
            warn!(domain = %domain_of(&email), "password setup request error (suppressed): {}", e);
        }
    } else {
        warn!(domain = %domain_of(&email), "password setup request rate limited; nothing sent");
    }
    Json(MessageResponse {
        message: SETUP_REQUESTED.to_string(),
    })
    .into_response()
}

/// Go `tryIssuePasswordSetupInvite`.
async fn try_issue_password_setup(
    state: &PasswordResetApiState,
    email: &str,
    redirect_uri: Option<&str>,
) -> Result<(), PlatformError> {
    if email.is_empty() {
        return Ok(());
    }
    let Some(principal) = state.principal_repo.find_by_email(email).await? else {
        return Ok(());
    };
    if !password_setup_eligible(&principal) {
        return Ok(());
    }
    // Only a domain that authenticates internally (unmapped, or not OIDC).
    if let Some(tf) = &state.two_factor {
        if !tf.policy.evaluate(email).await.internal {
            return Ok(());
        }
    }
    let redirect = redirect_uri.and_then(safe_relative_return_url);
    state.emailer.send_invite(&principal, redirect).await
}

/// `/auth/password-setup/*`.
pub fn password_setup_router(state: PasswordResetApiState) -> Router {
    Router::new()
        .route("/request", post(request_password_setup))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── hash_token tests ──

    #[test]
    fn hash_token_produces_hex_sha256() {
        let hash = hash_token("test-token-value");
        // SHA-256 hex is always 64 characters
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hash_token_is_deterministic() {
        let h1 = hash_token("same-input");
        let h2 = hash_token("same-input");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_token_different_inputs_differ() {
        let h1 = hash_token("token-a");
        let h2 = hash_token("token-b");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_token_empty_input() {
        let hash = hash_token("");
        // SHA-256 of empty string is e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    // ── generate_raw_token tests ──

    #[test]
    fn generate_raw_token_produces_non_empty_string() {
        let token = generate_raw_token();
        assert!(!token.is_empty());
    }

    #[test]
    fn generate_raw_token_is_url_safe_base64() {
        let token = generate_raw_token();
        // URL-safe base64 chars: A-Z, a-z, 0-9, -, _
        assert!(
            token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "Token contains non-URL-safe characters: {token}"
        );
    }

    #[test]
    fn generate_raw_token_has_correct_length() {
        let token = generate_raw_token();
        // 32 bytes -> base64 no-pad -> ceil(32 * 4/3) = 43 characters
        assert_eq!(
            token.len(),
            43,
            "Expected 43 chars for 32 bytes base64 no-pad, got {}",
            token.len()
        );
    }

    #[test]
    fn generate_raw_token_is_unique() {
        let t1 = generate_raw_token();
        let t2 = generate_raw_token();
        assert_ne!(t1, t2);
    }

    #[test]
    fn generated_token_hashes_to_valid_sha256() {
        let raw = generate_raw_token();
        let hashed = hash_token(&raw);
        assert_eq!(hashed.len(), 64);
        assert!(hashed.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ── DTO deserialization tests ──

    #[test]
    fn request_reset_body_deserializes() {
        let json = r#"{"email": "user@example.com"}"#;
        let body: RequestResetBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.email, "user@example.com");
    }

    #[test]
    fn request_reset_body_missing_email_fails() {
        let json = r#"{}"#;
        let result = serde_json::from_str::<RequestResetBody>(json);
        assert!(result.is_err());
    }

    #[test]
    fn confirm_reset_body_deserializes() {
        let json = r#"{"token": "abc123", "password": "NewP@ssw0rd!!"}"#;
        let body: ConfirmResetBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.token, "abc123");
        assert_eq!(body.password, "NewP@ssw0rd!!");
    }

    #[test]
    fn confirm_reset_body_missing_password_fails() {
        let json = r#"{"token": "abc123"}"#;
        let result = serde_json::from_str::<ConfirmResetBody>(json);
        assert!(result.is_err());
    }

    #[test]
    fn confirm_reset_body_missing_token_fails() {
        let json = r#"{"password": "NewP@ssw0rd!!"}"#;
        let result = serde_json::from_str::<ConfirmResetBody>(json);
        assert!(result.is_err());
    }

    #[test]
    fn validate_token_query_deserializes() {
        let json = r#"{"token": "my-token"}"#;
        let q: ValidateTokenQuery = serde_json::from_str(json).unwrap();
        assert_eq!(q.token, "my-token");
    }

    #[test]
    fn validate_token_response_serializes_valid() {
        let resp = ValidateTokenResponse {
            valid: true,
            reason: None,
            requires_factor: false,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["valid"], true);
        assert!(json["reason"].is_null());
    }

    #[test]
    fn validate_token_response_serializes_invalid() {
        let resp = ValidateTokenResponse {
            valid: false,
            reason: Some("expired".to_string()),
            requires_factor: false,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["valid"], false);
        assert_eq!(json["reason"], "expired");
    }
}
