//! Delete Client Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::ClientRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ClientDeleted;

/// Command for deleting a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteClientCommand {
    pub client_id: String,
}

pub struct DeleteClientUseCase<U: UnitOfWork> {
    client_repo: Arc<ClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteClientUseCase<U> {
    pub fn new(client_repo: Arc<ClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self { client_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: DeleteClientCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ClientDeleted> {
        if command.client_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "CLIENT_ID_REQUIRED", "Client ID is required",
            ));
        }

        let client = match self.client_repo.find_by_id(&command.client_id).await {
            Ok(Some(c)) => c,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "CLIENT_NOT_FOUND",
                    format!("Client with ID '{}' not found", command.client_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch client: {}", e
                )));
            }
        };

        let event = ClientDeleted::new(&ctx, &client.id, &client.name, &client.identifier);

        self.unit_of_work.commit_delete(&client, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeleteClientCommand {
            client_id: "client-123".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("clientId"));
    }
}
