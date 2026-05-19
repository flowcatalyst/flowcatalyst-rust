//! WebAuthn HTTP routes — `/auth/webauthn/*`.
//!
//! Six endpoints: register/begin, register/complete, authenticate/begin,
//! authenticate/complete, list credentials, delete credential.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use time::Duration as TimeDuration;
use tracing::warn;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;
use webauthn_rs::prelude::{PublicKeyCredential, RegisterPublicKeyCredential};

use crate::auth::login_backoff::{self, BackoffDecision, BackoffPolicy};
use crate::shared::error::PlatformError;
use crate::shared::middleware::{Authenticated, ClientIp};
use crate::usecase::ExecutionContext;
use crate::usecase::{PgUnitOfWork, UseCase};
use crate::webauthn::ceremony_repository::WebauthnCeremonyRepository;
use crate::webauthn::gate::ensure_internal_principal;
use crate::webauthn::operations::{
    AuthenticatePasskeyCommand, AuthenticatePasskeyUseCase, RegisterPasskeyCommand,
    RegisterPasskeyUseCase, RevokePasskeyCommand, RevokePasskeyUseCase,
};
use crate::webauthn::repository::WebauthnCredentialRepository;
use crate::webauthn::webauthn_service::WebauthnService;
use crate::{
    AttemptType, AuthService, LoginAttempt, LoginAttemptRepository, LoginOutcome,
    PrincipalRepository,
};

#[derive(Clone)]
pub struct WebauthnApiState {
    pub credential_repo: Arc<WebauthnCredentialRepository>,
    pub ceremony_repo: Arc<WebauthnCeremonyRepository>,
    pub principal_repo: Arc<PrincipalRepository>,
    pub login_attempt_repo: Arc<LoginAttemptRepository>,
    pub webauthn_service: Arc<WebauthnService>,
    pub auth_service: Arc<AuthService>,
    pub backoff_policy: Arc<BackoffPolicy>,
    pub unit_of_work: Arc<PgUnitOfWork>,
    pub session_cookie_name: String,
    pub session_cookie_secure: bool,
    pub session_cookie_same_site: String,
    pub session_token_expiry_secs: i64,
}

async fn record_login_attempt(
    repo: &LoginAttemptRepository,
    identifier: Option<&str>,
    principal_id: Option<&str>,
    ip: &str,
    outcome: LoginOutcome,
    failure_reason: Option<&str>,
) {
    let mut attempt = LoginAttempt::new(AttemptType::UserLogin, outcome);
    attempt.identifier = identifier.map(String::from);
    attempt.principal_id = principal_id.map(String::from);
    attempt.failure_reason = failure_reason.map(String::from);
    if !ip.is_empty() {
        attempt.ip_address = Some(ip.to_string());
    }

    if let Err(e) = repo.create(&attempt).await {
        warn!(error = %e, "Failed to record passkey login attempt (non-blocking)");
    }
}

// ── Request/response shapes ──────────────────────────────────────────────────

/// Optional metadata for a passkey registration ceremony.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegisterBeginRequest {
    /// Display name shown in the authenticator UI (defaults to the user's name).
    pub display_name: Option<String>,
}

/// WebAuthn registration challenge to hand to `navigator.credentials.create()`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegisterBeginResponse {
    /// Opaque ceremony state token; pass back unchanged on `register/complete`.
    pub state_id: String,
    /// `PublicKeyCredentialCreationOptions` JSON for the browser.
    #[schema(value_type = Object)]
    pub options: serde_json::Value,
}

/// Browser's registration response, plus the ceremony state token.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegisterCompleteRequest {
    pub state_id: String,
    /// User-supplied label (e.g. "Andrew's iPhone").
    pub name: Option<String>,
    /// The `PublicKeyCredential` returned by `navigator.credentials.create()`.
    #[schema(value_type = Object)]
    pub credential: RegisterPublicKeyCredential,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegisterCompleteResponse {
    pub credential_id: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateBeginRequest {
    pub email: String,
}

/// WebAuthn authentication challenge to hand to `navigator.credentials.get()`.
/// The shape is identical for known and unknown emails — see the enumeration
/// defence note in the source.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateBeginResponse {
    pub state_id: String,
    #[schema(value_type = Object)]
    pub options: serde_json::Value,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateCompleteRequest {
    pub state_id: String,
    /// The `PublicKeyCredential` returned by `navigator.credentials.get()`.
    #[schema(value_type = Object)]
    pub credential: PublicKeyCredential,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateCompleteResponse {
    pub principal_id: String,
    pub email: Option<String>,
    pub name: String,
    pub roles: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CredentialSummary {
    pub id: String,
    pub name: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn invalid_credentials() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({
            "error": "INVALID_CREDENTIALS",
            "message": "passkey authentication failed",
        })),
    )
        .into_response()
}

fn build_session_cookie(state: &WebauthnApiState, token: String) -> Cookie<'static> {
    let same_site = match state.session_cookie_same_site.to_lowercase().as_str() {
        "strict" => SameSite::Strict,
        "none" => SameSite::None,
        _ => SameSite::Lax,
    };
    Cookie::build((state.session_cookie_name.clone(), token))
        .path("/")
        .http_only(true)
        .secure(state.session_cookie_secure)
        .same_site(same_site)
        .max_age(TimeDuration::seconds(state.session_token_expiry_secs))
        .build()
}

// ── Routes ──────────────────────────────────────────────────────────────────

/// Begin passkey registration
///
/// Returns a WebAuthn `PublicKeyCredentialCreationOptions` challenge. The
/// browser passes this to `navigator.credentials.create()` and posts the
/// result to `/auth/webauthn/register/complete`.
#[utoipa::path(
    post,
    path = "/webauthn/register/begin",
    tag = "webauthn",
    operation_id = "postWebauthnRegisterBegin",
    request_body = RegisterBeginRequest,
    responses(
        (status = 200, description = "Registration challenge issued", body = RegisterBeginResponse),
        (status = 400, description = "Domain is federated or email malformed"),
        (status = 401, description = "Authentication required")
    )
)]
pub async fn register_begin(
    State(state): State<WebauthnApiState>,
    auth: Authenticated,
    Json(req): Json<RegisterBeginRequest>,
) -> Result<Json<RegisterBeginResponse>, PlatformError> {
    let email = auth
        .0
        .email
        .clone()
        .ok_or_else(|| PlatformError::bad_request("session has no email"))?;
    ensure_internal_principal(&email, &state.principal_repo).await?;

    let display_name = req
        .display_name
        .clone()
        .unwrap_or_else(|| auth.0.name.clone());

    // Don't let a user register the same authenticator twice — exclude their
    // existing credential ids from the challenge.
    let existing = state
        .credential_repo
        .find_by_principal(&auth.0.principal_id)
        .await?;
    let exclude: Vec<_> = existing
        .iter()
        .map(|c| c.passkey.cred_id().clone())
        .collect();

    let (challenge, ceremony_state) = state.webauthn_service.start_registration(
        &auth.0.principal_id,
        &email,
        &display_name,
        &exclude,
    )?;

    let state_id = Uuid::new_v4().to_string();
    state
        .ceremony_repo
        .store_registration(
            &state_id,
            &auth.0.principal_id,
            &ceremony_state,
            Some(&display_name),
        )
        .await?;

    let options = serde_json::to_value(&challenge)
        .map_err(|e| PlatformError::internal(format!("serialise challenge: {}", e)))?;

    Ok(Json(RegisterBeginResponse { state_id, options }))
}

/// Complete passkey registration
///
/// Validates the browser's attestation response and stores the credential.
#[utoipa::path(
    post,
    path = "/webauthn/register/complete",
    tag = "webauthn",
    operation_id = "postWebauthnRegisterComplete",
    request_body = RegisterCompleteRequest,
    responses(
        (status = 200, description = "Passkey registered", body = RegisterCompleteResponse),
        (status = 400, description = "Ceremony state expired or attestation invalid"),
        (status = 401, description = "Authentication required"),
        (status = 403, description = "Ceremony belongs to a different principal")
    )
)]
pub async fn register_complete(
    State(state): State<WebauthnApiState>,
    auth: Authenticated,
    Json(req): Json<RegisterCompleteRequest>,
) -> Result<Json<RegisterCompleteResponse>, PlatformError> {
    let consumed = state
        .ceremony_repo
        .consume_registration(&req.state_id)
        .await?
        .ok_or_else(|| {
            PlatformError::bad_request("registration ceremony state not found or expired")
        })?;

    if consumed.principal_id != auth.0.principal_id {
        return Err(PlatformError::Forbidden {
            message: "registration ceremony belongs to a different principal".to_string(),
        });
    }

    let use_case = RegisterPasskeyUseCase::new(
        state.credential_repo.clone(),
        state.webauthn_service.clone(),
        state.unit_of_work.clone(),
    );

    let ctx = ExecutionContext::from_auth(&auth.0);
    let cmd = RegisterPasskeyCommand {
        principal_id: consumed.principal_id,
        name: req.name,
        registration_response: req.credential,
        registration_state: Some(consumed.state),
    };

    let event = use_case.run(cmd, ctx).await.into_result()?;
    Ok(Json(RegisterCompleteResponse {
        credential_id: event.credential_id,
    }))
}

/// Begin passkey authentication
///
/// Returns a `PublicKeyCredentialRequestOptions` challenge. The response
/// shape is identical for known and unknown emails (deterministic-fake
/// `allowCredentials` is generated for unknown / federated / no-credentials
/// cases) — clients cannot distinguish them.
#[utoipa::path(
    post,
    path = "/webauthn/authenticate/begin",
    tag = "webauthn",
    operation_id = "postWebauthnAuthenticateBegin",
    request_body = AuthenticateBeginRequest,
    responses(
        (status = 200, description = "Authentication challenge issued", body = AuthenticateBeginResponse)
    )
)]
pub async fn authenticate_begin(
    State(state): State<WebauthnApiState>,
    Json(req): Json<AuthenticateBeginRequest>,
) -> Result<Json<AuthenticateBeginResponse>, PlatformError> {
    // Match-Google enumeration defence: the response shape is identical for
    // (a) real internal user with credentials, (b) federated user, (c) unknown
    // email, and (d) internal user with no credentials. Cases (b)–(d) get a
    // deterministic fake `allowCredentials` list seeded by an HMAC of the
    // email — so requests for the same email return the same shape, but an
    // attacker can't distinguish real vs. fake without the secret key.
    let real_credentials_opt = resolve_real_credentials(&state, &req.email).await;

    match real_credentials_opt {
        Some(passkeys) if !passkeys.is_empty() => {
            let (challenge, ceremony_state) =
                state.webauthn_service.start_authentication(&passkeys)?;
            let state_id = Uuid::new_v4().to_string();
            state
                .ceremony_repo
                .store_authentication(&state_id, None, &ceremony_state)
                .await?;
            let options = serde_json::to_value(&challenge)
                .map_err(|e| PlatformError::internal(format!("serialise challenge: {}", e)))?;
            Ok(Json(AuthenticateBeginResponse { state_id, options }))
        }
        _ => {
            // No real state stored — /complete will see "state not found"
            // and return the same INVALID_CREDENTIALS as a failed assertion.
            let state_id = Uuid::new_v4().to_string();
            let options = state
                .webauthn_service
                .fake_authentication_challenge(&req.email)?;
            Ok(Json(AuthenticateBeginResponse { state_id, options }))
        }
    }
}

async fn resolve_real_credentials(
    state: &WebauthnApiState,
    email: &str,
) -> Option<Vec<webauthn_rs::prelude::Passkey>> {
    if ensure_internal_principal(email, &state.principal_repo)
        .await
        .is_err()
    {
        return None;
    }
    let principal = state
        .principal_repo
        .find_by_email(email)
        .await
        .ok()
        .flatten()?;
    if !principal.active {
        return None;
    }
    let creds = state
        .credential_repo
        .find_by_principal(&principal.id)
        .await
        .ok()?;
    Some(creds.into_iter().map(|c| c.passkey).collect())
}

/// Complete passkey authentication
///
/// Validates the assertion, applies counter / backup-state updates,
/// re-checks the federation gate (hard cutover), and on success issues a
/// session cookie. All failure modes return 401 `INVALID_CREDENTIALS` with
/// an identical shape to defeat enumeration.
#[utoipa::path(
    post,
    path = "/webauthn/authenticate/complete",
    tag = "webauthn",
    operation_id = "postWebauthnAuthenticateComplete",
    request_body = AuthenticateCompleteRequest,
    responses(
        (status = 200, description = "Login successful, session cookie set", body = AuthenticateCompleteResponse),
        (status = 401, description = "Invalid credentials")
    )
)]
pub async fn authenticate_complete(
    State(state): State<WebauthnApiState>,
    ClientIp(client_ip): ClientIp,
    jar: CookieJar,
    Json(req): Json<AuthenticateCompleteRequest>,
) -> Response {
    let ip = client_ip.unwrap_or_default();

    let consumed = match state
        .ceremony_repo
        .consume_authentication(&req.state_id)
        .await
    {
        Ok(Some(c)) => c,
        _ => {
            record_login_attempt(
                &state.login_attempt_repo,
                None,
                None,
                &ip,
                LoginOutcome::Failure,
                Some("STATE_NOT_FOUND"),
            )
            .await;
            return invalid_credentials();
        }
    };

    let use_case = AuthenticatePasskeyUseCase::new(
        state.credential_repo.clone(),
        state.principal_repo.clone(),
        state.webauthn_service.clone(),
        state.unit_of_work.clone(),
    );

    // No caller identity at this stage — the execute() will resolve the
    // principal once the credential is loaded; for tracing/event metadata
    // we'll start as anonymous and the use case can re-bind.
    let ctx = ExecutionContext::create("anonymous");
    let cmd = AuthenticatePasskeyCommand {
        authentication_response: req.credential,
        authentication_state: Some(consumed.state),
    };

    let event = match use_case.run(cmd, ctx).await.into_result() {
        Ok(e) => e,
        Err(e) => {
            warn!(error = %e, "passkey authentication failed");
            // Distinguish federation/cutover from generic assertion failures
            // for security ops, without leaking distinguishability to the
            // caller — the response is identical.
            let reason = if format!("{}", e).contains("DOMAIN_FEDERATED") {
                "DOMAIN_FEDERATED"
            } else if format!("{}", e).contains("PRINCIPAL_INACTIVE") {
                "ACCOUNT_INACTIVE"
            } else {
                "INVALID_CREDENTIALS"
            };
            record_login_attempt(
                &state.login_attempt_repo,
                None,
                None,
                &ip,
                LoginOutcome::Failure,
                Some(reason),
            )
            .await;
            return invalid_credentials();
        }
    };

    let principal = match state.principal_repo.find_by_id(&event.principal_id).await {
        Ok(Some(p)) => p,
        _ => {
            record_login_attempt(
                &state.login_attempt_repo,
                None,
                Some(&event.principal_id),
                &ip,
                LoginOutcome::Failure,
                Some("PRINCIPAL_NOT_FOUND"),
            )
            .await;
            return invalid_credentials();
        }
    };

    // Apply the backoff/global-ceiling gate now that we know the email.
    // Belt-and-braces: passkeys are infeasible to brute-force, but a
    // determined attacker could chase fallback flows; locking out the same
    // email across both /auth/login and the passkey path closes that gap.
    if let Some(email) = principal.email() {
        match login_backoff::check(&state.login_attempt_repo, &state.backoff_policy, email, &ip)
            .await
        {
            Ok(BackoffDecision::Allow) => {}
            Ok(BackoffDecision::Reject {
                retry_after_secs, ..
            }) => {
                record_login_attempt(
                    &state.login_attempt_repo,
                    Some(email),
                    Some(&principal.id),
                    &ip,
                    LoginOutcome::Failure,
                    Some("RATE_LIMITED"),
                )
                .await;
                return login_backoff::rejection_error(retry_after_secs).into_response();
            }
            Err(e) => {
                warn!(error = %e, "backoff check failed; continuing fail-open");
            }
        }
    }

    let token = match state.auth_service.generate_session_token(&principal) {
        Ok(t) => t,
        Err(e) => {
            warn!(error = %e, "failed to generate session token after passkey login");
            record_login_attempt(
                &state.login_attempt_repo,
                principal.email(),
                Some(&principal.id),
                &ip,
                LoginOutcome::Failure,
                Some("SESSION_TOKEN_FAILED"),
            )
            .await;
            return invalid_credentials();
        }
    };

    let cookie = build_session_cookie(&state, token);
    let jar = jar.add(cookie);

    record_login_attempt(
        &state.login_attempt_repo,
        principal.email(),
        Some(&principal.id),
        &ip,
        LoginOutcome::Success,
        None,
    )
    .await;

    let response = AuthenticateCompleteResponse {
        principal_id: principal.id.clone(),
        email: principal.email().map(String::from),
        name: principal.name.clone(),
        roles: principal.roles.iter().map(|r| r.role.clone()).collect(),
    };
    (jar, Json(response)).into_response()
}

/// List the caller's registered passkeys
#[utoipa::path(
    get,
    path = "/webauthn/credentials",
    tag = "webauthn",
    operation_id = "getWebauthnCredentials",
    responses(
        (status = 200, description = "Caller's passkeys", body = Vec<CredentialSummary>),
        (status = 401, description = "Authentication required")
    )
)]
pub async fn list_credentials(
    State(state): State<WebauthnApiState>,
    auth: Authenticated,
) -> Result<Json<Vec<CredentialSummary>>, PlatformError> {
    let creds = state
        .credential_repo
        .find_by_principal(&auth.0.principal_id)
        .await?;
    let summaries = creds
        .into_iter()
        .map(|c| CredentialSummary {
            id: c.id,
            name: c.name,
            created_at: c.created_at,
            last_used_at: c.last_used_at,
        })
        .collect();
    Ok(Json(summaries))
}

/// Revoke one of the caller's passkeys
#[utoipa::path(
    delete,
    path = "/webauthn/credentials/{id}",
    tag = "webauthn",
    operation_id = "deleteWebauthnCredential",
    params(("id" = String, Path, description = "Credential id (pkc_…)")),
    responses(
        (status = 204, description = "Passkey revoked"),
        (status = 401, description = "Authentication required"),
        (status = 404, description = "Credential not found or not owned by caller")
    )
)]
pub async fn delete_credential(
    State(state): State<WebauthnApiState>,
    auth: Authenticated,
    Path(credential_id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    let use_case =
        RevokePasskeyUseCase::new(state.credential_repo.clone(), state.unit_of_work.clone());
    let ctx = ExecutionContext::from_auth(&auth.0);
    let cmd = RevokePasskeyCommand { credential_id };
    use_case.run(cmd, ctx).await.into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Router ──────────────────────────────────────────────────────────────────

pub fn webauthn_router(state: WebauthnApiState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(register_begin))
        .routes(routes!(register_complete))
        .routes(routes!(authenticate_begin))
        .routes(routes!(authenticate_complete))
        .routes(routes!(list_credentials))
        .routes(routes!(delete_credential))
        .with_state(state)
}
