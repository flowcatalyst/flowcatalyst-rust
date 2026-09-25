//! Activate Client Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ClientActivated;
use crate::client::entity::{Client, ClientStatus};
use crate::client::repository::ClientRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

/// Command for activating a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivateClientCommand {
    /// Client ID to activate
    pub client_id: String,
}

impl crate::usecase::AuditMasked for ActivateClientCommand {}

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

    async fn authorize(
        &self,
        _command: &ActivateClientCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        // Authorization handled in handler
        Ok(())
    }

    async fn execute(
        &self,
        command: ActivateClientCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ClientActivated> {
        let (client, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // Atomic commit
        self.unit_of_work
            .commit(&client, &*self.client_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> ActivateClientUseCase<U> {
    async fn prepare(
        &self,
        command: &ActivateClientCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Client, ClientActivated), UseCaseError> {
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
        let event = ClientActivated::new(ctx, &client.id);
        Ok((client, event))
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
