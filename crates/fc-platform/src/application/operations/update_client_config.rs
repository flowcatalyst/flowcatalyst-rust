//! Update Application-for-Client Config Use Case.
//!
//! PUT /api/applications/{id}/clients/{client_id} — mutate the per-client
//! config for an application (enabled flag / base URL override / arbitrary
//! config json). All writes commit atomically via UoW with an
//! `ApplicationClientConfigUpdated` event + audit log.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ApplicationClientConfigUpdated;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ApplicationClientConfig;
use crate::ApplicationClientConfigRepository;
use crate::ApplicationRepository;
use crate::ClientRepository;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApplicationClientConfigCommand {
    pub application_id: String,
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// `Some("")` means "clear the override"; `None` means "leave as-is".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url_override: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

pub struct UpdateApplicationClientConfigUseCase<U: UnitOfWork> {
    application_repo: Arc<ApplicationRepository>,
    client_repo: Arc<ClientRepository>,
    config_repo: Arc<ApplicationClientConfigRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateApplicationClientConfigUseCase<U> {
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
impl<U: UnitOfWork> UseCase for UpdateApplicationClientConfigUseCase<U> {
    type Command = UpdateApplicationClientConfigCommand;
    type Event = ApplicationClientConfigUpdated;

    async fn validate(
        &self,
        command: &UpdateApplicationClientConfigCommand,
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
        _command: &UpdateApplicationClientConfigCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateApplicationClientConfigCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ApplicationClientConfigUpdated> {
        let (config, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&config, &*self.config_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> UpdateApplicationClientConfigUseCase<U> {
    async fn prepare(
        &self,
        command: &UpdateApplicationClientConfigCommand,
        ctx: &ExecutionContext,
    ) -> Result<(ApplicationClientConfig, ApplicationClientConfigUpdated), UseCaseError> {
        // Verify application exists
        self.application_repo
            .find_by_id(&command.application_id)
            .await
            .or_not_found(
                "APPLICATION_NOT_FOUND",
                format!("Application '{}' not found", command.application_id),
            )?;

        // Verify client exists
        self.client_repo
            .find_by_id(&command.client_id)
            .await
            .or_not_found(
                "CLIENT_NOT_FOUND",
                format!("Client '{}' not found", command.client_id),
            )?;

        // Load-or-create the config and apply the patch
        let mut config = self
            .config_repo
            .find_by_application_and_client(&command.application_id, &command.client_id)
            .await?
            .unwrap_or_else(|| {
                ApplicationClientConfig::new(&command.application_id, &command.client_id)
            });

        if let Some(enabled) = command.enabled {
            config.enabled = enabled;
        }
        if let Some(ref url) = command.base_url_override {
            config.base_url_override = if url.is_empty() {
                None
            } else {
                Some(url.clone())
            };
        }
        let config_changed = command.config.is_some();
        if let Some(ref cfg) = command.config {
            config.config_json = Some(cfg.clone());
        }
        config.updated_at = chrono::Utc::now();

        let event = ApplicationClientConfigUpdated {
            metadata: ApplicationClientConfigUpdated::metadata_for(ctx, &command.application_id),
            application_id: command.application_id.clone(),
            client_id: command.client_id.clone(),
            config_id: config.id.clone(),
            enabled: command.enabled,
            base_url_override: command.base_url_override.clone(),
            config_changed,
        };
        Ok((config, event))
    }
}
