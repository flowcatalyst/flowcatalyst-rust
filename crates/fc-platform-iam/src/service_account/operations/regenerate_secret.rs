//! Regenerate Signing Secret Use Case

use async_trait::async_trait;
use chrono::Utc;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ServiceAccountSecretRegenerated;
use crate::service_account::repository::ServiceAccountRepository;
use fc_platform_core::shared::encryption_service::{require_configured, EncryptionService};
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Generate a signing secret (URL-safe base64)
fn generate_signing_secret() -> String {
    let bytes: [u8; 32] = rand::rng().random();
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
}

/// Command for regenerating a service account's signing secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegenerateSigningSecretCommand {
    /// Service account ID
    pub service_account_id: String,
}

impl fc_platform_core::usecase::AuditMasked for RegenerateSigningSecretCommand {}

/// Result returned from regenerate signing secret use case.
/// Contains the event plus one-time secret that needs to be returned to caller.
/// The secret is never serialized, so this serializes exactly as the event.
#[derive(Serialize)]
pub struct RegenerateSigningSecretResult {
    #[serde(flatten)]
    pub event: ServiceAccountSecretRegenerated,
    #[serde(skip_serializing)]
    pub signing_secret: String,
}

fc_platform_core::impl_domain_event!(RegenerateSigningSecretResult => event);

/// Use case for regenerating a service account's signing secret.
pub struct RegenerateSigningSecretUseCase<U: UnitOfWork> {
    service_account_repo: Arc<ServiceAccountRepository>,
    unit_of_work: Arc<U>,
    /// Encrypts the generated credential before it is stored. `None` when no
    /// key is configured; the use case then fails rather than store plaintext.
    encryption: Option<Arc<EncryptionService>>,
}

impl<U: UnitOfWork> RegenerateSigningSecretUseCase<U> {
    pub fn new(
        service_account_repo: Arc<ServiceAccountRepository>,
        unit_of_work: Arc<U>,
        encryption: Option<Arc<EncryptionService>>,
    ) -> Self {
        Self {
            service_account_repo,
            unit_of_work,
            encryption,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RegenerateSigningSecretUseCase<U> {
    type Command = RegenerateSigningSecretCommand;
    type Event = RegenerateSigningSecretResult;

    async fn validate(
        &self,
        _command: &RegenerateSigningSecretCommand,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    /// Service accounts are written by anchors only: an account's tier follows
    /// its client links, so a non-anchor could otherwise mint an ANCHOR-tier
    /// account (the `can_*_service_accounts` rules).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &RegenerateSigningSecretCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(
            fc_platform_core::shared::authorization_service::checks::require_anchor_scope(
                ctx.caller(),
            )?,
        )
    }

    async fn execute(
        &self,
        command: RegenerateSigningSecretCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<RegenerateSigningSecretResult>, UseCaseError> {
        // Find the service account
        let mut service_account = self
            .service_account_repo
            .find_by_id(&command.service_account_id)
            .await
            .or_not_found(
                "SERVICE_ACCOUNT_NOT_FOUND",
                format!(
                    "Service account with ID '{}' not found",
                    command.service_account_id
                ),
            )?;

        // Generate new secret; the caller gets the plaintext once, only the
        // `encrypted:` form is stored.
        let signing_secret = generate_signing_secret();
        let signing_secret_ref =
            require_configured(self.encryption.as_deref())?.encrypt_ref(&signing_secret)?;
        service_account.webhook_credentials.signing_secret = Some(signing_secret_ref);
        service_account.updated_at = Utc::now();

        // Create domain event
        let event = ServiceAccountSecretRegenerated::new(&ctx, &service_account);

        // Create result with one-time secret
        let result = RegenerateSigningSecretResult {
            event: event.clone(),
            signing_secret,
        };

        // Atomic commit through UnitOfWork, then map the event onto our
        // wrapper (carrying the one-time secret).
        self.unit_of_work
            .commit(
                &service_account,
                &*self.service_account_repo,
                event,
                &command,
            )
            .await
            .map(|committed| committed.map(|_| result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = RegenerateSigningSecretCommand {
            service_account_id: "sa-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("sa-123"));
    }

    #[test]
    fn test_generate_signing_secret() {
        let secret = generate_signing_secret();
        assert!(!secret.is_empty());
        // URL-safe base64 of 32 bytes should be ~43 chars
        assert!(secret.len() > 40);
    }
}
