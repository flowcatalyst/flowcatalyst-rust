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
use crate::principal::repository::PrincipalRepository;
use crate::principal::operations::events::PasswordResetCompleted;
use crate::auth::password_service::PasswordService;
use crate::shared::error::PlatformError;
use crate::{PgUnitOfWork, UnitOfWork};

#[derive(Clone)]
pub struct PasswordResetApiState {
    pub password_reset_repo: Arc<PasswordResetTokenRepository>,
    pub principal_repo: Arc<PrincipalRepository>,
    pub password_service: Arc<PasswordService>,
    pub unit_of_work: Arc<PgUnitOfWork>,
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
    // Silent success pattern: always return the same message regardless of whether the email exists
    let result: Result<(), PlatformError> = async {
        let principal = state.principal_repo.find_by_email(&body.email).await?;
        if let Some(principal) = principal {
            // Delete any existing tokens for this principal
            state.password_reset_repo.delete_by_principal_id(&principal.id).await?;

            // Create a new token
            let raw_token = generate_raw_token();
            let token_hash = hash_token(&raw_token);
            let expires_at = Utc::now() + Duration::minutes(15);

            let reset_token = PasswordResetToken::new(
                &principal.id,
                token_hash,
                expires_at,
            );
            state.password_reset_repo.create(&reset_token).await?;

            // In production, send an email with the raw_token here.
            // For now, log it (development convenience).
            info!(
                principal_id = %principal.id,
                "Password reset token created (raw token not sent — email sending not implemented)"
            );
        } else {
            // Principal not found — do nothing (silent success)
            warn!(email = %body.email, "Password reset requested for unknown email");
        }
        Ok(())
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
