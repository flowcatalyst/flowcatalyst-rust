//! Regenerate Auth Token Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};
use chrono::Utc;
use rand::Rng;

use crate::WebhookAuthType;
use crate::ServiceAccountRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ServiceAccountTokenRegenerated;

/// Generate a bearer token with fc_ prefix
fn generate_auth_token() -> String {
    let random_part: String = (0..32)
        .map(|_| {
            let idx = rand::thread_rng().gen_range(0..36);
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
pub struct RegenerateAuthTokenResult {
    pub event: ServiceAccountTokenRegenerated,
    pub auth_token: String,
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

    pub async fn execute(
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

        // Atomic commit
        match self.unit_of_work.commit(&service_account, event, &command).await {
            UseCaseResult::Success(_) => UseCaseResult::success(result),
            UseCaseResult::Failure(e) => UseCaseResult::Failure(e),
        }
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
