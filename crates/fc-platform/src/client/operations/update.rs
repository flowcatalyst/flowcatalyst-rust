//! Update Client Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::client::repository::ClientRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ClientUpdated;

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

    pub async fn execute(
        &self,
        command: UpdateClientCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ClientUpdated> {
        // Validation: client_id is required
        if command.client_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "CLIENT_ID_REQUIRED",
                "Client ID is required",
            ));
        }

        // Validation: at least one field to update
        if command.name.is_none() {
            return UseCaseResult::failure(UseCaseError::validation(
                "NO_UPDATES",
                "At least one field must be provided for update",
            ));
        }

        // Validation: name if provided
        if let Some(ref name) = command.name {
            let name = name.trim();
            if name.is_empty() {
                return UseCaseResult::failure(UseCaseError::validation(
                    "NAME_REQUIRED",
                    "Client name cannot be empty",
                ));
            }
            if name.len() > 100 {
                return UseCaseResult::failure(UseCaseError::validation(
                    "NAME_TOO_LONG",
                    "Client name must be at most 100 characters",
                ));
            }
        }

        // Fetch existing client
        let mut client = match self.client_repo.find_by_id(&command.client_id).await {
            Ok(Some(c)) => c,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "CLIENT_NOT_FOUND",
                    format!("Client with ID '{}' not found", command.client_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch client: {}",
                    e
                )));
            }
        };

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
            return UseCaseResult::failure(UseCaseError::validation(
                "NO_CHANGES",
                "No changes detected",
            ));
        }

        client.updated_at = chrono::Utc::now();

        // Create domain event
        let event = ClientUpdated::new(
            &ctx,
            &client.id,
            updated_name,
            None,
        );

        // Atomic commit
        self.unit_of_work.commit(&client, event, &command).await
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
