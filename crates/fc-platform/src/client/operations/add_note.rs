//! Add Client Note Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ClientNoteAdded;
use crate::client::entity::{Client, ClientNote};
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ClientRepository;

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
        Self {
            client_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for AddClientNoteUseCase<U> {
    type Command = AddClientNoteCommand;
    type Event = ClientNoteAdded;

    async fn validate(&self, command: &AddClientNoteCommand) -> Result<(), UseCaseError> {
        if command.client_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CLIENT_ID_REQUIRED",
                "Client ID is required",
            ));
        }

        if command.category.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CATEGORY_REQUIRED",
                "Note category is required",
            ));
        }

        if command.text.trim().is_empty() {
            return Err(UseCaseError::validation(
                "TEXT_REQUIRED",
                "Note text is required",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &AddClientNoteCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        // Authorization handled in handler
        Ok(())
    }

    async fn execute(
        &self,
        command: AddClientNoteCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ClientNoteAdded> {
        let (client, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&client, &*self.client_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> AddClientNoteUseCase<U> {
    async fn prepare(
        &self,
        command: &AddClientNoteCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Client, ClientNoteAdded), UseCaseError> {
        let category = command.category.trim();
        let text = command.text.trim();

        let mut client = self
            .client_repo
            .find_by_id(&command.client_id)
            .await
            .or_not_found(
                "CLIENT_NOT_FOUND",
                format!("Client with ID '{}' not found", command.client_id),
            )?;

        let note = ClientNote::new(category, text).with_author(&ctx.principal_id);
        client.add_note(note);

        let event = ClientNoteAdded::new(ctx, &client.id, category, text, &ctx.principal_id);
        Ok((client, event))
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
