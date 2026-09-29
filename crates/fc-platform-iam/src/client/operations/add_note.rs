//! Add Client Note Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ClientNoteAdded;
use crate::client::entity::ClientNote;
use crate::client::repository::ClientRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for adding a note to a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddClientNoteCommand {
    pub client_id: String,
    pub category: String,
    pub text: String,
}

impl AuditMasked for AddClientNoteCommand {}

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

    /// Clients are platform-owner data, written by anchors only (Go's
    /// `Can*Clients` are `anchorWith`).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &AddClientNoteCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: AddClientNoteCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ClientNoteAdded>, UseCaseError> {
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

        let event = ClientNoteAdded::new(&ctx, client.id.as_str(), category, text);

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
