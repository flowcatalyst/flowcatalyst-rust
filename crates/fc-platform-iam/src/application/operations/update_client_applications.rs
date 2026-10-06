//! Update Client Applications Use Case
//!
//! Bulk update of which applications are enabled for a given client. Computes
//! the diff against the current state, persists every changed config row in
//! one transaction, and emits a single `ClientApplicationsUpdated` event
//! summarising the diff. Replaces N enable/disable round-trips that each
//! emitted their own event.

use fc_platform_core::shared::id::ApplicationId;
use fc_platform_core::shared::id::ClientId;
use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::events::ClientApplicationsUpdated;
use crate::application::client_config::ApplicationClientConfig;
use crate::application::client_config_repository::ApplicationClientConfigRepository;
use crate::application::repository::ApplicationRepository;
use crate::client::repository::ClientRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for replacing the enabled-application set for a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateClientApplicationsCommand {
    pub client_id: ClientId,
    /// Authoritative list. Apps in here become enabled; existing enabled apps
    /// not in here become disabled.
    pub enabled_application_ids: Vec<ApplicationId>,
}

impl AuditMasked for UpdateClientApplicationsCommand {}

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
        _command: &UpdateClientApplicationsCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: UpdateClientApplicationsCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ClientApplicationsUpdated>, UseCaseError> {
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

        let desired: HashSet<&ApplicationId> = command.enabled_application_ids.iter().collect();
        let currently_enabled: HashSet<&ApplicationId> = current_configs
            .iter()
            .filter(|c| c.enabled)
            .map(|c| &c.application_id)
            .collect();

        let mut to_persist: Vec<ApplicationClientConfig> = Vec::new();
        let mut enabled_added: Vec<ApplicationId> = Vec::new();
        let mut disabled_removed: Vec<ApplicationId> = Vec::new();

        // Enable: requested but not currently enabled. Either flip an existing
        // disabled row, or create a fresh enabled row.
        for app_id in &command.enabled_application_ids {
            if currently_enabled.contains(app_id) {
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
                None => ApplicationClientConfig::new(app_id.clone(), command.client_id.clone()),
            };
            to_persist.push(cfg);
            enabled_added.push(app_id.clone());
        }

        // Disable: currently enabled but not requested.
        for cfg in &current_configs {
            if cfg.enabled && !desired.contains(&cfg.application_id) {
                let mut c = cfg.clone();
                c.disable();
                disabled_removed.push(c.application_id.clone());
                to_persist.push(c);
            }
        }

        let event = ClientApplicationsUpdated::new(
            &ctx,
            &command.client_id,
            command.enabled_application_ids.clone(),
            enabled_added,
            disabled_removed,
        );

        // No diff → still emit one event so the audit trail records the request.
        if to_persist.is_empty() {
            return self.unit_of_work.emit_event(event, &command).await;
        }

        self.unit_of_work
            .commit_all(&to_persist, &*self.config_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = UpdateClientApplicationsCommand {
            client_id: ClientId::parse("clt_123").unwrap(),
            enabled_application_ids: vec![
                ApplicationId::parse("app_a").unwrap(),
                ApplicationId::parse("app_b").unwrap(),
            ],
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("clientId"));
        assert!(json.contains("enabledApplicationIds"));
    }
}
