//! Regenerate Auth Token Use Case

use async_trait::async_trait;
use chrono::Utc;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ServiceAccountTokenRegenerated;
use crate::service_account::ServiceAccount;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ServiceAccountRepository;
use crate::WebhookAuthType;

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
/// The token is never serialized, so this serializes exactly as the event.
#[derive(Serialize)]
pub struct RegenerateAuthTokenResult {
    #[serde(flatten)]
    pub event: ServiceAccountTokenRegenerated,
    #[serde(skip_serializing)]
    pub auth_token: String,
}

crate::impl_domain_event!(RegenerateAuthTokenResult => event);

/// Use case for regenerating a service account's auth token.
pub struct RegenerateAuthTokenUseCase<U: UnitOfWork> {
    service_account_repo: Arc<ServiceAccountRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> RegenerateAuthTokenUseCase<U> {
    pub fn new(service_account_repo: Arc<ServiceAccountRepository>, unit_of_work: Arc<U>) -> Self {
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

    async fn authorize(
        &self,
        _command: &RegenerateAuthTokenCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: RegenerateAuthTokenCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<RegenerateAuthTokenResult> {
        let (service_account, event, result) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // Atomic commit through UnitOfWork, then map the event onto our
        // wrapper (carrying the one-time token).
        self.unit_of_work
            .commit(
                &service_account,
                &*self.service_account_repo,
                event,
                &command,
            )
            .await
            .map(|_| result)
    }
}

impl<U: UnitOfWork> RegenerateAuthTokenUseCase<U> {
    async fn prepare(
        &self,
        command: &RegenerateAuthTokenCommand,
        ctx: &ExecutionContext,
    ) -> Result<
        (
            ServiceAccount,
            ServiceAccountTokenRegenerated,
            RegenerateAuthTokenResult,
        ),
        UseCaseError,
    > {
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

        // Generate new token
        let auth_token = generate_auth_token();
        service_account.webhook_credentials.token = Some(auth_token.clone());
        service_account.webhook_credentials.auth_type = WebhookAuthType::BearerToken;
        service_account.updated_at = Utc::now();

        // Create domain event
        let event =
            ServiceAccountTokenRegenerated::new(ctx, &service_account.id, &service_account.code);

        // Create result with one-time token
        let result = RegenerateAuthTokenResult {
            event: event.clone(),
            auth_token,
        };

        Ok((service_account, event, result))
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
