//! Enable Application for Client Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ApplicationEnabledForClient;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ApplicationClientConfig;
use crate::ApplicationClientConfigRepository;
use crate::ApplicationRepository;
use crate::ClientRepository;

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
        Self {
            application_repo,
            client_repo,
            config_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for EnableApplicationForClientUseCase<U> {
    type Command = EnableApplicationForClientCommand;
    type Event = ApplicationEnabledForClient;

    async fn validate(
        &self,
        command: &EnableApplicationForClientCommand,
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
        _command: &EnableApplicationForClientCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: EnableApplicationForClientCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ApplicationEnabledForClient> {
        let (config, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&config, &*self.config_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> EnableApplicationForClientUseCase<U> {
    async fn prepare(
        &self,
        command: &EnableApplicationForClientCommand,
        ctx: &ExecutionContext,
    ) -> Result<(ApplicationClientConfig, ApplicationEnabledForClient), UseCaseError> {
        // Validate application exists
        self.application_repo
            .find_by_id(&command.application_id)
            .await
            .or_not_found(
                "APPLICATION_NOT_FOUND",
                format!("Application '{}' not found", command.application_id),
            )?;

        // Validate client exists
        self.client_repo
            .find_by_id(&command.client_id)
            .await
            .or_not_found(
                "CLIENT_NOT_FOUND",
                format!("Client '{}' not found", command.client_id),
            )?;

        // Check if config already exists
        let config = match self
            .config_repo
            .find_by_application_and_client(&command.application_id, &command.client_id)
            .await?
        {
            Some(mut cfg) => {
                // Idempotent: enable if disabled
                cfg.enable();
                cfg
            }
            None => {
                // Create new config
                ApplicationClientConfig::new(&command.application_id, &command.client_id)
            }
        };

        let event = ApplicationEnabledForClient::new(
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
        let cmd = EnableApplicationForClientCommand {
            application_id: "app-123".to_string(),
            client_id: "client-456".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("applicationId"));
    }
}
