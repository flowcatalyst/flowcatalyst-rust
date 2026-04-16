//! /auth/password-reset Routes — Password reset flow (unauthenticated)

use axum::{
    routing::{get, post},
    extract::{State, Query},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use std::sync::Arc;
use chrono::{Utc, Duration};
use sha2::{Sha256, Digest};
use tracing::{info, warn};

use crate::password_reset::entity::PasswordResetToken;
use crate::password_reset::repository::PasswordResetTokenRepository;
use crate::principal::entity::Principal;
use crate::principal::repository::PrincipalRepository;
use crate::principal::operations::events::{PasswordResetCompleted, PasswordResetRequested};
use crate::auth::password_service::PasswordService;
use crate::shared::error::PlatformError;
use crate::shared::email_service::{EmailService, EmailMessage};
use crate::{PgUnitOfWork, UnitOfWork};

/// Shared service that creates a single-use reset token and emails the
/// recipient with a link back to the SPA. Used by both the user-initiated
/// `/auth/password-reset/request` flow and the admin-initiated
/// `/api/principals/{id}/send-password-reset` action.
#[derive(Clone)]
pub struct PasswordResetEmailer {
    pub password_reset_repo: Arc<PasswordResetTokenRepository>,
    pub email_service: Arc<dyn EmailService>,
    pub unit_of_work: Arc<PgUnitOfWork>,
    /// Base URL for constructing reset links (e.g. "https://app.flowcatalyst.io")
    pub external_base_url: String,
}

impl PasswordResetEmailer {
    /// Generate a single-use token, persist it (15 min TTL), and email the
    /// principal a reset link. Email failures are logged but not propagated
    /// (best-effort delivery; the token is still valid for direct use).
    ///
    /// Caller is responsible for validating that `principal` is eligible for
    /// password reset (USER type, has email, not OIDC-federated). This method
    /// expects `principal.user_identity.email` to be present.
    pub async fn send_reset_email(&self, principal: &Principal) -> Result<(), PlatformError> {
        let email = principal.user_identity.as_ref()
            .map(|i| i.email.clone())
            .ok_or_else(|| PlatformError::validation(
                "Principal does not have an email address for password reset",
            ))?;

        // Invalidate any outstanding tokens for this principal.
        self.password_reset_repo.delete_by_principal_id(&principal.id).await?;

        let raw_token = generate_raw_token();
        let token_hash = hash_token(&raw_token);
        let expires_at = Utc::now() + Duration::minutes(15);

        let reset_token = PasswordResetToken::new(&principal.id, token_hash, expires_at);
        self.password_reset_repo.create(&reset_token).await?;

        // Email link must match the SPA's `/auth/reset-password` route
        // (frontend/src/router/index.ts). The API namespace
        // `/auth/password-reset/*` is *not* a frontend route.
        let reset_link = format!(
            "{}/auth/reset-password?token={}",
            self.external_base_url, raw_token
        );
        let message = EmailMessage {
            to: email.clone(),
            subject: "Reset your password".to_string(),
            html_body: format!(
                "<p>You requested a password reset.</p>\
                 <p><a href=\"{}\">Click here to reset your password</a></p>\
                 <p>This link expires in 15 minutes.</p>\
                 <p>If you did not request this, you can safely ignore this email.</p>",
                reset_link
            ),
            text_body: Some(format!(
                "You requested a password reset.\n\nReset link: {}\n\nThis link expires in 15 minutes.",
                reset_link
            )),
        };
        if let Err(e) = self.email_service.send(&message).await {
            warn!(principal_id = %principal.id, error = %e, "Failed to send password reset email");
        }

        // Best-effort domain event.
        let event = PasswordResetRequested::new(&principal.id, &email);
        let command = serde_json::json!({ "principalId": principal.id, "email": email });
        if let Err(e) = self.unit_of_work.emit_event(event, &command).await.into_result() {
            warn!("Failed to emit PasswordResetRequested event: {}", e);
        }

        Ok(())
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
}

// -- Request / Response DTOs --

#[derive(Debug, Deserialize, ToSchema)]
pub struct RequestResetBody {
    pub email: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ValidateTokenQuery {
    pub token: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ValidateTokenResponse {
    pub valid: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ConfirmResetBody {
    pub token: String,
    pub password: String,
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
    // Silent success pattern: always return the same message regardless of
    // whether the email exists, so the endpoint can't be used to enumerate
    // accounts.
    let result: Result<(), PlatformError> = async {
        match state.principal_repo.find_by_email(&body.email).await? {
            Some(principal) => state.emailer.send_reset_email(&principal).await,
            None => {
                warn!(email = %body.email, "Password reset requested for unknown email");
                Ok(())
            }
        }
    }.await;

    if let Err(e) = result {
        warn!("Password reset request error (suppressed): {}", e);
    }

    Json(MessageResponse {
        message: "If an account exists, a reset email has been sent.".to_string(),
    })
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
    let token_hash = hash_token(&query.token);

    match state.password_reset_repo.find_by_token_hash(&token_hash).await {
        Ok(Some(token)) => {
            if token.is_expired() {
                Json(ValidateTokenResponse { valid: false, reason: Some("expired".to_string()) })
            } else {
                Json(ValidateTokenResponse { valid: true, reason: None })
            }
        }
        Ok(None) => {
            Json(ValidateTokenResponse { valid: false, reason: Some("not_found".to_string()) })
        }
        Err(e) => {
            warn!("Token validation error: {}", e);
            Json(ValidateTokenResponse { valid: false, reason: Some("not_found".to_string()) })
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
        (status = 200, description = "Password reset successfully", body = MessageResponse),
        (status = 400, description = "Invalid or expired token")
    )
)]
async fn confirm_reset(
    State(state): State<PasswordResetApiState>,
    Json(body): Json<ConfirmResetBody>,
) -> Result<Json<MessageResponse>, PlatformError> {
    let token_hash = hash_token(&body.token);

    let reset_token = state.password_reset_repo.find_by_token_hash(&token_hash).await?
        .ok_or_else(|| PlatformError::Validation {
            message: "Invalid or expired reset token.".to_string(),
        })?;

    if reset_token.is_expired() {
        // Clean up the expired token
        let _ = state.password_reset_repo.delete_by_id(&reset_token.id).await;
        return Err(PlatformError::Validation {
            message: "Reset token has expired.".to_string(),
        });
    }

    // Validate and hash the new password
    let password_hash = state.password_service.hash_password(&body.password)?;

    // Update the principal's password
    let mut principal = state.principal_repo.find_by_id(&reset_token.principal_id).await?
        .ok_or_else(|| PlatformError::Validation {
            message: "Associated account not found.".to_string(),
        })?;

    if let Some(ref mut identity) = principal.user_identity {
        identity.password_hash = Some(password_hash);
    } else {
        return Err(PlatformError::Validation {
            message: "Account does not support password authentication.".to_string(),
        });
    }
    state.principal_repo.update(&principal).await?;

    // Delete all reset tokens for this principal (consumed)
    state.password_reset_repo.delete_by_principal_id(&reset_token.principal_id).await?;

    // Emit domain event (best-effort, non-blocking)
    let email = principal.user_identity
        .as_ref()
        .map(|id| id.email.as_str())
        .unwrap_or("")
        .to_string();
    let event = PasswordResetCompleted::new(&reset_token.principal_id, &email);
    let command = serde_json::json!({ "principalId": reset_token.principal_id });
    if let Err(e) = state.unit_of_work.emit_event(event, &command).await.into_result() {
        warn!("Failed to emit PasswordResetCompleted event (reset still succeeded): {}", e);
    }

    info!(principal_id = %reset_token.principal_id, "Password reset completed successfully");

    Ok(Json(MessageResponse {
        message: "Password reset successfully.".to_string(),
    }))
}

pub fn password_reset_router(state: PasswordResetApiState) -> Router {
    Router::new()
        .route("/request", post(request_reset))
        .route("/validate", get(validate_token))
        .route("/confirm", post(confirm_reset))
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
        assert_eq!(hash, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
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
            token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "Token contains non-URL-safe characters: {token}"
        );
    }

    #[test]
    fn generate_raw_token_has_correct_length() {
        let token = generate_raw_token();
        // 32 bytes -> base64 no-pad -> ceil(32 * 4/3) = 43 characters
        assert_eq!(token.len(), 43, "Expected 43 chars for 32 bytes base64 no-pad, got {}", token.len());
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
        let resp = ValidateTokenResponse { valid: true, reason: None };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["valid"], true);
        assert!(json["reason"].is_null());
    }

    #[test]
    fn validate_token_response_serializes_invalid() {
        let resp = ValidateTokenResponse { valid: false, reason: Some("expired".to_string()) };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["valid"], false);
        assert_eq!(json["reason"], "expired");
    }
}
