//! Activate Client Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ClientActivated;
use crate::client::entity::ClientStatus;
use crate::client::repository::ClientRepository;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for activating a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivateClientCommand {
    /// Client ID to activate
    pub client_id: String,
}

impl fc_platform_core::usecase::AuditMasked for ActivateClientCommand {}

/// Use case for activating a suspended or pending client.
pub struct ActivateClientUseCase<U: UnitOfWork> {
    client_repo: Arc<ClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> ActivateClientUseCase<U> {
    pub fn new(client_repo: Arc<ClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            client_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for ActivateClientUseCase<U> {
    type Command = ActivateClientCommand;
    type Event = ClientActivated;

    async fn validate(&self, command: &ActivateClientCommand) -> Result<(), UseCaseError> {
        if command.client_id.trim().is_empty() {
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
        _command: &ActivateClientCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(
            fc_platform_core::shared::authorization_service::checks::require_anchor_scope(
                ctx.caller(),
            )?,
        )
    }

    async fn execute(
        &self,
        command: ActivateClientCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ClientActivated>, UseCaseError> {
        // Fetch existing client
        let mut client = self
            .client_repo
            .find_by_id(&command.client_id)
            .await
            .or_not_found(
                "CLIENT_NOT_FOUND",
                format!("Client with ID '{}' not found", command.client_id),
            )?;

        // Business rule: client must not already be active
        if client.status == ClientStatus::Active {
            return Err(UseCaseError::business_rule(
                "ALREADY_ACTIVE",
                "Client is already active",
            ));
        }

        // Activate the client
        client.activate();

        // Create domain event
        let event = ClientActivated::new(&ctx, &client.id);

        // Atomic commit
        self.unit_of_work
            .commit(&client, &*self.client_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = ActivateClientCommand {
            client_id: "client-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("clientId"));
        assert!(json.contains("client-123"));
    }
}
