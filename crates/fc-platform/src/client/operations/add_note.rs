//! Add Client Note Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::client::entity::ClientNote;
use crate::ClientRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ClientNoteAdded;

/// Command for adding a note to a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddClientNoteCommand {
    pub client_id: String,
    pub category: String,
    pub text: String,
}

pub struct AddClientNoteUseCase<U: UnitOfWork> {
    client_repo: Arc<ClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> AddClientNoteUseCase<U> {
    pub fn new(client_repo: Arc<ClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self { client_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: AddClientNoteCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ClientNoteAdded> {
        if command.client_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "CLIENT_ID_REQUIRED", "Client ID is required",
            ));
        }

        let category = command.category.trim();
        if category.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "CATEGORY_REQUIRED", "Note category is required",
            ));
        }

        let text = command.text.trim();
        if text.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "TEXT_REQUIRED", "Note text is required",
            ));
        }

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
                    "Failed to fetch client: {}", e
                )));
            }
        };

        let note = ClientNote::new(category, text)
            .with_author(&ctx.principal_id);
        client.add_note(note);

        let event = ClientNoteAdded::new(
            &ctx,
            &client.id,
            category,
            text,
            &ctx.principal_id,
        );

        self.unit_of_work.commit(&client, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = AddClientNoteCommand {
            client_id: "client-123".to_string(),
            category: "general".to_string(),
            text: "Important note".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("clientId"));
        assert!(json.contains("general"));
    }
}
