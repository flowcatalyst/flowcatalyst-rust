//! Regenerate Auth Token Use Case

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use chrono::Utc;
use rand::Rng;

use crate::WebhookAuthType;
use crate::ServiceAccountRepository;
use crate::usecase::{
    ExecutionContext, UseCase, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ServiceAccountTokenRegenerated;

/// Generate a bearer token with fc_ prefix
fn generate_auth_token() -> String {
    let random_part: String = (0..32)
        .map(|_| {
            let idx = rand::rng().random_range(0..36);
            if idx < 10 {
                (b'0' + idx) as char
            } else {
                (b'a' + idx - 10) as char
            }
        })
        .collect();
    format!("fc_{}", random_part)
}

/// Command for regenerating a service account's auth token.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegenerateAuthTokenCommand {
    /// Service account ID
    pub service_account_id: String,
}

/// Result returned from regenerate auth token use case.
/// Contains the event plus one-time token that needs to be returned to caller.
#[derive(Serialize)]
pub struct RegenerateAuthTokenResult {
    #[serde(flatten)]
    pub event: ServiceAccountTokenRegenerated,
    pub auth_token: String,
}

impl crate::usecase::DomainEvent for RegenerateAuthTokenResult {
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

/// Use case for regenerating a service account's auth token.
pub struct RegenerateAuthTokenUseCase<U: UnitOfWork> {
    service_account_repo: Arc<ServiceAccountRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> RegenerateAuthTokenUseCase<U> {
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
impl<U: UnitOfWork> UseCase for RegenerateAuthTokenUseCase<U> {
    type Command = RegenerateAuthTokenCommand;
    type Event = RegenerateAuthTokenResult;

    async fn validate(&self, _command: &RegenerateAuthTokenCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn authorize(&self, _command: &RegenerateAuthTokenCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: RegenerateAuthTokenCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<RegenerateAuthTokenResult> {
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

        // Generate new token
        let auth_token = generate_auth_token();
        service_account.webhook_credentials.token = Some(auth_token.clone());
        service_account.webhook_credentials.auth_type = WebhookAuthType::BearerToken;
        service_account.updated_at = Utc::now();

        // Create domain event
        let event = ServiceAccountTokenRegenerated::new(
            &ctx,
            &service_account.id,
            &service_account.code,
        );

        // Create result with one-time token
        let result = RegenerateAuthTokenResult {
            event: event.clone(),
            auth_token,
        };

        // Atomic commit through UnitOfWork, then map the event onto our
        // wrapper (carrying the one-time token).
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
        let cmd = RegenerateAuthTokenCommand {
            service_account_id: "sa-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("sa-123"));
    }

    #[test]
    fn test_generate_auth_token() {
        let token = generate_auth_token();
        assert!(token.starts_with("fc_"));
        assert_eq!(token.len(), 35);
    }
}
