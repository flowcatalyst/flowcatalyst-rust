//! Disable Application for Client Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::ApplicationClientConfigRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ApplicationDisabledForClient;

/// Command for disabling an application for a specific client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisableApplicationForClientCommand {
    pub application_id: String,
    pub client_id: String,
}

pub struct DisableApplicationForClientUseCase<U: UnitOfWork> {
    config_repo: Arc<ApplicationClientConfigRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DisableApplicationForClientUseCase<U> {
    pub fn new(
        config_repo: Arc<ApplicationClientConfigRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self { config_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: DisableApplicationForClientCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ApplicationDisabledForClient> {
        if command.application_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "APPLICATION_ID_REQUIRED", "Application ID is required",
            ));
        }
        if command.client_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "CLIENT_ID_REQUIRED", "Client ID is required",
            ));
        }

        // Find existing config
        let mut config = match self.config_repo
            .find_by_application_and_client(&command.application_id, &command.client_id)
            .await
        {
            Ok(Some(cfg)) => cfg,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "CONFIG_NOT_FOUND",
                    "Application is not configured for this client",
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch config: {}", e
                )));
            }
        };

        // Idempotent: disable
        config.disable();

        let event = ApplicationDisabledForClient::new(
            &ctx,
            &command.application_id,
            &command.client_id,
            &config.id,
        );

        self.unit_of_work.commit(&config, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DisableApplicationForClientCommand {
            application_id: "app-123".to_string(),
            client_id: "client-456".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("applicationId"));
    }
}
