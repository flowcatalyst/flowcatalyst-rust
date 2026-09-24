//! Update Client Applications Use Case
//!
//! Bulk update of which applications are enabled for a given client. Computes
//! the diff against the current state, persists every changed config row in
//! one transaction, and emits a single `ClientApplicationsUpdated` event
//! summarising the diff. Replaces N enable/disable round-trips that each
//! emitted their own event.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::events::ClientApplicationsUpdated;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ApplicationClientConfig;
use crate::ApplicationClientConfigRepository;
use crate::ApplicationRepository;
use crate::ClientRepository;

/// Command for replacing the enabled-application set for a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateClientApplicationsCommand {
    pub client_id: String,
    /// Authoritative list. Apps in here become enabled; existing enabled apps
    /// not in here become disabled.
    pub enabled_application_ids: Vec<String>,
}

pub struct UpdateClientApplicationsUseCase<U: UnitOfWork> {
    application_repo: Arc<ApplicationRepository>,
    client_repo: Arc<ClientRepository>,
    config_repo: Arc<ApplicationClientConfigRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateClientApplicationsUseCase<U> {
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
impl<U: UnitOfWork> UseCase for UpdateClientApplicationsUseCase<U> {
    type Command = UpdateClientApplicationsCommand;
    type Event = ClientApplicationsUpdated;

    async fn validate(
        &self,
        command: &UpdateClientApplicationsCommand,
    ) -> Result<(), UseCaseError> {
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
        _command: &UpdateClientApplicationsCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateClientApplicationsCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ClientApplicationsUpdated> {
        let (to_persist, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // No diff → still emit one event so the audit trail records the request.
        if to_persist.is_empty() {
            return self.unit_of_work.emit_event(event, &command).await;
        }

        self.unit_of_work
            .commit_all(&to_persist, &*self.config_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> UpdateClientApplicationsUseCase<U> {
    async fn prepare(
        &self,
        command: &UpdateClientApplicationsCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Vec<ApplicationClientConfig>, ClientApplicationsUpdated), UseCaseError> {
        // 1. Client must exist.
        self.client_repo
            .find_by_id(&command.client_id)
            .await
            .or_not_found(
                "CLIENT_NOT_FOUND",
                format!("Client '{}' not found", command.client_id),
            )?;

        // 2. Every requested application must exist (batch existence check).
        for app_id in &command.enabled_application_ids {
            if !self.application_repo.exists(app_id).await? {
                return Err(UseCaseError::not_found(
                    "APPLICATION_NOT_FOUND",
                    format!("Application '{}' not found", app_id),
                ));
            }
        }

        // 3. Load current configs to compute the diff.
        let current_configs = self.config_repo.find_by_client(&command.client_id).await?;

        let desired: HashSet<&str> = command
            .enabled_application_ids
            .iter()
            .map(String::as_str)
            .collect();
        let currently_enabled: HashSet<&str> = current_configs
            .iter()
            .filter(|c| c.enabled)
            .map(|c| c.application_id.as_str())
            .collect();

        let mut to_persist: Vec<ApplicationClientConfig> = Vec::new();
        let mut enabled_added: Vec<String> = Vec::new();
        let mut disabled_removed: Vec<String> = Vec::new();

        // Enable: requested but not currently enabled. Either flip an existing
        // disabled row, or create a fresh enabled row.
        for app_id in &command.enabled_application_ids {
            if currently_enabled.contains(app_id.as_str()) {
                continue;
            }
            let existing = current_configs
                .iter()
                .find(|c| c.application_id == *app_id)
                .cloned();
            let cfg = match existing {
                Some(mut c) => {
                    c.enable();
                    c
                }
                None => ApplicationClientConfig::new(app_id, &command.client_id),
            };
            to_persist.push(cfg);
            enabled_added.push(app_id.clone());
        }

        // Disable: currently enabled but not requested.
        for cfg in &current_configs {
            if cfg.enabled && !desired.contains(cfg.application_id.as_str()) {
                let mut c = cfg.clone();
                c.disable();
                disabled_removed.push(c.application_id.clone());
                to_persist.push(c);
            }
        }

        let event = ClientApplicationsUpdated::new(
            ctx,
            &command.client_id,
            command.enabled_application_ids.clone(),
            enabled_added,
            disabled_removed,
        );

        Ok((to_persist, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = UpdateClientApplicationsCommand {
            client_id: "clt_123".to_string(),
            enabled_application_ids: vec!["app_a".to_string(), "app_b".to_string()],
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("clientId"));
        assert!(json.contains("enabledApplicationIds"));
    }
}
