//! Deactivate Client Use Case

use async_trait::async_trait;
use fc_platform_core::shared::id::ClientId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ClientDeactivated;
use crate::client::repository::ClientRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for deactivating a client. The reason is optional.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeactivateClientCommand {
    /// Client ID to deactivate
    pub client_id: ClientId,

    /// Why (kept as the client's status reason; blank is none)
    #[serde(default)]
    pub reason: String,
}

impl AuditMasked for DeactivateClientCommand {}

/// Marks a client INACTIVE: a soft delete. The row and its configuration
/// stay, and `ActivateClientUseCase` reactivates it; `DeleteClientUseCase`
/// is the irreversible removal (Go `DeactivateClient`, bug 15).
pub struct DeactivateClientUseCase<U: UnitOfWork> {
    client_repo: Arc<ClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeactivateClientUseCase<U> {
    pub fn new(client_repo: Arc<ClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            client_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeactivateClientUseCase<U> {
    type Command = DeactivateClientCommand;
    type Event = ClientDeactivated;

    async fn validate(&self, command: &DeactivateClientCommand) -> Result<(), UseCaseError> {
        if command.client_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation("ID_REQUIRED", "id is required"));
        }
        Ok(())
    }

    /// Clients are platform-owner data, written by anchors only (Go's
    /// `Can*Clients` are `anchorWith`). The handler's gate checks this, with
    /// the permission, before the body is read; here it holds for every
    /// caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &DeactivateClientCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: DeactivateClientCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ClientDeactivated>, UseCaseError> {
        let mut client = self
            .client_repo
            .find_by_id(&command.client_id)
            .await
            .or_not_found(
                "CLIENT_NOT_FOUND",
                format!("Client with ID '{}' not found", command.client_id),
            )?;

        let reason = command.reason.trim();
        client.deactivate(Some(reason.to_string()).filter(|r| !r.is_empty()));

        let event = ClientDeactivated::new(&ctx, &client.id, reason);

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
        let cmd = DeactivateClientCommand {
            client_id: ClientId::from_wire("client-123"),
            reason: "Manual deactivation".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("clientId"));
        assert!(json.contains("Manual deactivation"));
    }
}
