//! Password reset and invite tokens: minted, stored hashed and e-mailed
//! (the self-service request routes, fc-platform-auth's
//! `password_reset_api`, and the admin send-reset and create-user paths).

use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tracing::warn;

use crate::password_reset::entity::{PasswordResetToken, TokenPurpose};
use crate::password_reset::repository::PasswordResetTokenRepository;
use crate::principal::entity::Principal;
use crate::principal::operations::events::PasswordResetRequested;
use crate::shared::branding::{EmailContent, Theme};
use fc_platform_core::shared::email_service::{EmailMessage, EmailService};
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::usecase::unit_of_work::{PgUnitOfWork, UnitOfWork};

/// A reset token's lifetime (15 minutes).
const RESET_TOKEN_TTL_MINUTES: i64 = 15;
/// An invite's lifetime (72 hours): time for a new user to act on it.
const INVITE_TOKEN_TTL_HOURS: i64 = 72;
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
    /// Platform config, for the login theme (logo, colours, brand name) the
    /// emails are rendered with (Go `NewEmailer(svc, brand)`); `None` renders
    /// the defaults.
    pub brand: Option<Arc<crate::platform_config::repository::PlatformConfigRepository>>,
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
        // Go `linkEmailer.SendResetLink`.
        let theme = Theme::load(self.brand.as_ref()).await;
        let message = EmailMessage {
            to: email.clone(),
            subject: "Reset your password".to_string(),
            html_body: theme.render_email(&EmailContent {
                heading: "Reset your password",
                intro: "We received a request to reset your password. Click the button below to choose a new one.",
                button_label: "Reset password",
                button_url: &reset_link,
                after_button: &[
                    "This link expires in 15 minutes.",
                    "If you didn't request this, you can safely ignore this email.",
                ],
                ..EmailContent::default()
            }),
            text_body: None,
        };
        if let Err(e) = self.email_service.send(&message).await {
            warn!(principal_id = %principal.id, error = %e, "Failed to send password reset email");
        }

        // Best-effort domain event.
        let event = PasswordResetRequested::new(&principal.id, &email);
        let command = serde_json::json!({ "principalId": principal.id, "email": email });
        if let Err(e) = self.unit_of_work.emit_event(event, &command).await {
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
        // Go `linkEmailer.SendInviteLink`.
        let theme = Theme::load(self.brand.as_ref()).await;
        let heading = format!("Welcome to {}", theme.brand_name);
        let message = EmailMessage {
            to: email,
            subject: "Set your password".to_string(),
            html_body: theme.render_email(&EmailContent {
                heading: &heading,
                intro: "An account has been created for you. Click the button below to set your password and sign in.",
                button_label: "Set your password",
                button_url: &link,
                after_button: &[
                    "If two-factor authentication is required for your organisation, you'll be guided through setting it up.",
                    "This link expires in 72 hours.",
                ],
                ..EmailContent::default()
            }),
            text_body: None,
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

/// Hash a raw token to produce the stored token_hash (SHA-256 hex).
pub fn hash_token(raw_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_token.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Generate a secure random token (URL-safe base64, 32 bytes).
pub fn generate_raw_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}
