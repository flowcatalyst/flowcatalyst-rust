//! Regenerate Signing Secret Use Case

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use chrono::Utc;
use rand::Rng;

use crate::ServiceAccountRepository;
use crate::usecase::{
    ExecutionContext, UseCase, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ServiceAccountSecretRegenerated;

/// Generate a signing secret (URL-safe base64)
fn generate_signing_secret() -> String {
    let bytes: [u8; 32] = rand::rng().random();
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &bytes)
}

/// Command for regenerating a service account's signing secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegenerateSigningSecretCommand {
    /// Service account ID
    pub service_account_id: String,
}

/// Result returned from regenerate signing secret use case.
/// Contains the event plus one-time secret that needs to be returned to caller.
#[derive(Serialize)]
pub struct RegenerateSigningSecretResult {
    #[serde(flatten)]
    pub event: ServiceAccountSecretRegenerated,
    pub signing_secret: String,
}

impl crate::usecase::DomainEvent for RegenerateSigningSecretResult {
    fn event_id(&self) -> &str { self.event.event_id() }
    fn event_type(&self) -> &str { self.event.event_type() }
    fn spec_version(&self) -> &str { self.event.spec_version() }
    fn source(&self) -> &str { self.event.source() }
    fn subject(&self) -> &str { self.event.subject() }
    fn time(&self) -> chrono::DateTime<chrono::Utc> { self.event.time() }
    fn execution_id(&self) -> &str { self.event.execution_id() }
    fn correlation_id(&self) -> &str { self.event.correlation_id() }
    fn causation_id(&self) -> Option<&str> { self.event.causation_id() }
    fn principal_id(&self) -> &str { self.event.principal_id() }
    fn message_group(&self) -> &str { self.event.message_group() }
    fn to_data_json(&self) -> String { self.event.to_data_json() }
}

/// Use case for regenerating a service account's signing secret.
pub struct RegenerateSigningSecretUseCase<U: UnitOfWork> {
    service_account_repo: Arc<ServiceAccountRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> RegenerateSigningSecretUseCase<U> {
    pub fn new(
        service_account_repo: Arc<ServiceAccountRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            service_account_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RegenerateSigningSecretUseCase<U> {
    type Command = RegenerateSigningSecretCommand;
    type Event = RegenerateSigningSecretResult;

    async fn validate(&self, _command: &RegenerateSigningSecretCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn authorize(&self, _command: &RegenerateSigningSecretCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: RegenerateSigningSecretCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<RegenerateSigningSecretResult> {
        // Find the service account
        let mut service_account = match self.service_account_repo.find_by_id(&command.service_account_id).await {
            Ok(Some(sa)) => sa,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "SERVICE_ACCOUNT_NOT_FOUND",
                    format!("Service account with ID '{}' not found", command.service_account_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(
                    format!("Failed to find service account: {}", e),
                ));
            }
        };

        // Generate new secret
        let signing_secret = generate_signing_secret();
        service_account.webhook_credentials.signing_secret = Some(signing_secret.clone());
        service_account.updated_at = Utc::now();

        // Create domain event
        let event = ServiceAccountSecretRegenerated::new(
            &ctx,
            &service_account.id,
            &service_account.code,
        );

        // Create result with one-time secret
        let result = RegenerateSigningSecretResult {
            event: event.clone(),
            signing_secret,
        };

        // Atomic commit through UnitOfWork, then map the event onto our
        // wrapper (carrying the one-time secret).
        self.unit_of_work
            .commit(&service_account, event, &command)
            .await
            .map(|_| result)
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
