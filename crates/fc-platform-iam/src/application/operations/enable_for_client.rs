//! Enable Application for Client Use Case

use async_trait::async_trait;
use fc_platform_core::shared::id::ApplicationId;
use fc_platform_core::shared::id::ClientId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ApplicationEnabledForClient;
use crate::application::client_config::ApplicationClientConfig;
use crate::application::client_config_repository::ApplicationClientConfigRepository;
use crate::application::repository::ApplicationRepository;
use crate::client::repository::ClientRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for enabling an application for a specific client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnableApplicationForClientCommand {
    pub application_id: ApplicationId,
    pub client_id: ClientId,
}

impl AuditMasked for EnableApplicationForClientCommand {}

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
        if command.application_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_ID_REQUIRED",
                "Application ID is required",
            ));
        }
        if command.client_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation(
                "CLIENT_ID_REQUIRED",
                "Client ID is required",
            ));
        }

        Ok(())
    }

    /// Clients are platform-owner data, written by anchors only (Go's
    /// `Can*Clients` are `anchorWith`).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &EnableApplicationForClientCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: EnableApplicationForClientCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ApplicationEnabledForClient>, UseCaseError> {
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
                ApplicationClientConfig::new(
                    command.application_id.clone(),
                    command.client_id.clone(),
                )
            }
        };

        let event = ApplicationEnabledForClient::new(
            &ctx,
            &command.application_id,
            &command.client_id,
            &config.id,
        );

        self.unit_of_work
            .commit(&config, &*self.config_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = EnableApplicationForClientCommand {
            application_id: ApplicationId::parse("app_123").unwrap(),
            client_id: ClientId::from_wire("client-456"),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("applicationId"));
    }
}
