//! Disable Application for Client Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ApplicationDisabledForClient;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ApplicationClientConfigRepository;

/// Command for disabling an application for a specific client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisableApplicationForClientCommand {
    pub application_id: String,
    pub client_id: String,
}

impl crate::usecase::AuditMasked for DisableApplicationForClientCommand {}

pub struct DisableApplicationForClientUseCase<U: UnitOfWork> {
    config_repo: Arc<ApplicationClientConfigRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DisableApplicationForClientUseCase<U> {
    pub fn new(config_repo: Arc<ApplicationClientConfigRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            config_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DisableApplicationForClientUseCase<U> {
    type Command = DisableApplicationForClientCommand;
    type Event = ApplicationDisabledForClient;

    async fn validate(
        &self,
        command: &DisableApplicationForClientCommand,
    ) -> Result<(), UseCaseError> {
        if command.application_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_ID_REQUIRED",
                "Application ID is required",
            ));
        }
        if command.client_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CLIENT_ID_REQUIRED",
                "Client ID is required",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &DisableApplicationForClientCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DisableApplicationForClientCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ApplicationDisabledForClient> {
        let (config, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&config, &*self.config_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> DisableApplicationForClientUseCase<U> {
    async fn prepare(
        &self,
        command: &DisableApplicationForClientCommand,
        ctx: &ExecutionContext,
    ) -> Result<(crate::ApplicationClientConfig, ApplicationDisabledForClient), UseCaseError> {
        // Find existing config
        let mut config = self
            .config_repo
            .find_by_application_and_client(&command.application_id, &command.client_id)
            .await
            .or_not_found(
                "CONFIG_NOT_FOUND",
                "Application is not configured for this client",
            )?;

        // Idempotent: disable
        config.disable();

        let event = ApplicationDisabledForClient::new(
            ctx,
            &command.application_id,
            &command.client_id,
            &config.id,
        );
        Ok((config, event))
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
