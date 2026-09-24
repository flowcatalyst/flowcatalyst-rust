//! Update Client Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ClientUpdated;
use crate::client::entity::Client;
use crate::client::repository::ClientRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

/// Command for updating an existing client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateClientCommand {
    /// Client ID to update
    pub client_id: String,

    /// New name (optional, 1-100 chars)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl crate::usecase::AuditMasked for UpdateClientCommand {}

/// Use case for updating an existing client.
pub struct UpdateClientUseCase<U: UnitOfWork> {
    client_repo: Arc<ClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateClientUseCase<U> {
    pub fn new(client_repo: Arc<ClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            client_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateClientUseCase<U> {
    type Command = UpdateClientCommand;
    type Event = ClientUpdated;

    async fn validate(&self, command: &UpdateClientCommand) -> Result<(), UseCaseError> {
        // client_id is required
        if command.client_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CLIENT_ID_REQUIRED",
                "Client ID is required",
            ));
        }

        // At least one field to update
        if command.name.is_none() {
            return Err(UseCaseError::validation(
                "NO_UPDATES",
                "At least one field must be provided for update",
            ));
        }

        // Name if provided
        if let Some(ref name) = command.name {
            let name = name.trim();
            if name.is_empty() {
                return Err(UseCaseError::validation(
                    "NAME_REQUIRED",
                    "Client name cannot be empty",
                ));
            }
            if name.len() > 100 {
                return Err(UseCaseError::validation(
                    "NAME_TOO_LONG",
                    "Client name must be at most 100 characters",
                ));
            }
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &UpdateClientCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        // Authorization handled in handler
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateClientCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ClientUpdated> {
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

impl<U: UnitOfWork> UpdateClientUseCase<U> {
    async fn prepare(
        &self,
        command: &UpdateClientCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Client, ClientUpdated), UseCaseError> {
        // Fetch existing client
        let mut client = self
            .client_repo
            .find_by_id(&command.client_id)
            .await
            .or_not_found(
                "CLIENT_NOT_FOUND",
                format!("Client with ID '{}' not found", command.client_id),
            )?;

        // Apply updates
        let mut updated_name: Option<&str> = None;

        if let Some(ref name) = command.name {
            let name = name.trim();
            if name != client.name {
                client.name = name.to_string();
                updated_name = Some(name);
            }
        }

        // Check if anything actually changed
        if updated_name.is_none() {
            return Err(UseCaseError::validation(
                "NO_CHANGES",
                "No changes detected",
            ));
        }

        client.updated_at = chrono::Utc::now();

        // Create domain event
        let event = ClientUpdated::new(ctx, &client.id, updated_name, None);
        Ok((client, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = UpdateClientCommand {
            client_id: "client-123".to_string(),
            name: Some("New Name".to_string()),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("clientId"));
        assert!(json.contains("New Name"));
    }
}
