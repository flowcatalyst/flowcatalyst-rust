//! Activate Client Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::client::entity::ClientStatus;
use crate::client::repository::ClientRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ClientActivated;

/// Command for activating a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivateClientCommand {
    /// Client ID to activate
    pub client_id: String,
}

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

    pub async fn execute(
        &self,
        command: ActivateClientCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ClientActivated> {
        // Validation: client_id is required
        if command.client_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "CLIENT_ID_REQUIRED",
                "Client ID is required",
            ));
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

        // Business rule: cannot activate a deleted client
        if client.status == ClientStatus::Deleted {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "CANNOT_ACTIVATE_DELETED",
                "Cannot activate a deleted client",
            ));
        }

        // Business rule: client must not already be active
        if client.status == ClientStatus::Active {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "ALREADY_ACTIVE",
                "Client is already active",
            ));
        }

        let previous_status = client.status;

        // Activate the client
        client.activate();

        // Create domain event
        let event = ClientActivated::new(&ctx, &client.id, previous_status);

        // Atomic commit
        self.unit_of_work.commit(&client, event, &command).await
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
