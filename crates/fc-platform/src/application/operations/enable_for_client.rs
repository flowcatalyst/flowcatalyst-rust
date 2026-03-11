//! Enable Application for Client Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::ApplicationRepository;
use crate::ClientRepository;
use crate::ApplicationClientConfigRepository;
use crate::ApplicationClientConfig;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ApplicationEnabledForClient;

/// Command for enabling an application for a specific client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnableApplicationForClientCommand {
    pub application_id: String,
    pub client_id: String,
}

pub struct EnableApplicationForClientUseCase<U: UnitOfWork> {
    application_repo: Arc<ApplicationRepository>,
    client_repo: Arc<ClientRepository>,
    config_repo: Arc<ApplicationClientConfigRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> EnableApplicationForClientUseCase<U> {
    pub fn new(
        application_repo: Arc<ApplicationRepository>,
        client_repo: Arc<ClientRepository>,
        config_repo: Arc<ApplicationClientConfigRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self { application_repo, client_repo, config_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: EnableApplicationForClientCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ApplicationEnabledForClient> {
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

        // Validate application exists
        match self.application_repo.find_by_id(&command.application_id).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "APPLICATION_NOT_FOUND",
                    format!("Application '{}' not found", command.application_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to validate application: {}", e
                )));
            }
        }

        // Validate client exists
        match self.client_repo.find_by_id(&command.client_id).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "CLIENT_NOT_FOUND",
                    format!("Client '{}' not found", command.client_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to validate client: {}", e
                )));
            }
        }

        // Check if config already exists
        let existing = self.config_repo
            .find_by_application_and_client(&command.application_id, &command.client_id)
            .await;

        let config = match existing {
            Ok(Some(mut cfg)) => {
                // Idempotent: enable if disabled
                cfg.enable();
                cfg
            }
            Ok(None) => {
                // Create new config
                ApplicationClientConfig::new(&command.application_id, &command.client_id)
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to check existing config: {}", e
                )));
            }
        };

        let event = ApplicationEnabledForClient::new(
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
        let cmd = EnableApplicationForClientCommand {
            application_id: "app-123".to_string(),
            client_id: "client-456".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("applicationId"));
    }
}
