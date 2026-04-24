//! Update Application-for-Client Config Use Case.
//!
//! PUT /api/applications/{id}/clients/{client_id} — mutate the per-client
//! config for an application (enabled flag / base URL override / arbitrary
//! config json). All writes commit atomically via UoW with an
//! `ApplicationClientConfigUpdated` event + audit log.

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::ApplicationClientConfig;
use crate::ApplicationClientConfigRepository;
use crate::ApplicationRepository;
use crate::ClientRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use super::events::ApplicationClientConfigUpdated;

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
        Self { application_repo, client_repo, config_repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateApplicationClientConfigUseCase<U> {
    type Command = UpdateApplicationClientConfigCommand;
    type Event = ApplicationClientConfigUpdated;

    async fn validate(&self, command: &UpdateApplicationClientConfigCommand) -> Result<(), UseCaseError> {
        if command.application_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_ID_REQUIRED", "Application ID is required",
            ));
        }
        if command.client_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CLIENT_ID_REQUIRED", "Client ID is required",
            ));
        }
        Ok(())
    }

    async fn authorize(&self, _command: &UpdateApplicationClientConfigCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateApplicationClientConfigCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ApplicationClientConfigUpdated> {
        // Verify application exists
        if self.application_repo.find_by_id(&command.application_id).await
            .map_err(|e| UseCaseError::commit(format!("fetch application: {}", e)))
            .and_then(|opt| opt.ok_or_else(|| UseCaseError::not_found(
                "APPLICATION_NOT_FOUND",
                format!("Application '{}' not found", command.application_id),
            )))
            .is_err()
        {
            // Re-run without collapsing to get the exact error back
            return match self.application_repo.find_by_id(&command.application_id).await {
                Ok(Some(_)) => unreachable!(),
                Ok(None) => UseCaseResult::failure(UseCaseError::not_found(
                    "APPLICATION_NOT_FOUND",
                    format!("Application '{}' not found", command.application_id),
                )),
                Err(e) => UseCaseResult::failure(UseCaseError::commit(format!(
                    "fetch application: {}", e,
                ))),
            };
        }

        // Verify client exists
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
                    "fetch client: {}", e,
                )));
            }
        }

        // Load-or-create the config and apply the patch
        let existing = self.config_repo
            .find_by_application_and_client(&command.application_id, &command.client_id)
            .await;

        let mut config = match existing {
            Ok(Some(cfg)) => cfg,
            Ok(None) => ApplicationClientConfig::new(&command.application_id, &command.client_id),
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "fetch config: {}", e,
                )));
            }
        };

        if let Some(enabled) = command.enabled {
            config.enabled = enabled;
        }
        if let Some(ref url) = command.base_url_override {
            config.base_url_override = if url.is_empty() { None } else { Some(url.clone()) };
        }
        let config_changed = command.config.is_some();
        if let Some(ref cfg) = command.config {
            config.config_json = Some(cfg.clone());
        }
        config.updated_at = chrono::Utc::now();

        let event = ApplicationClientConfigUpdated::new(
            &ctx,
            &command.application_id,
            &command.client_id,
            &config.id,
            command.enabled,
            command.base_url_override.clone(),
            config_changed,
        );

        self.unit_of_work
            .commit(&config, &*self.config_repo, event, &command)
            .await
    }
}
