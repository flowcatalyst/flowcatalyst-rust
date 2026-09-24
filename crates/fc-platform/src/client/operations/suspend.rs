//! Suspend Client Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ClientSuspended;
use crate::client::entity::{Client, ClientStatus};
use crate::client::repository::ClientRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

/// Command for suspending a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuspendClientCommand {
    /// Client ID to suspend
    pub client_id: String,

    /// Reason for suspension (required, 1-500 chars)
    pub reason: String,
}

/// Use case for suspending an active client.
pub struct SuspendClientUseCase<U: UnitOfWork> {
    client_repo: Arc<ClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SuspendClientUseCase<U> {
    pub fn new(client_repo: Arc<ClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            client_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SuspendClientUseCase<U> {
    type Command = SuspendClientCommand;
    type Event = ClientSuspended;

    async fn validate(&self, command: &SuspendClientCommand) -> Result<(), UseCaseError> {
        if command.client_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CLIENT_ID_REQUIRED",
                "Client ID is required",
            ));
        }

        let reason = command.reason.trim();
        if reason.is_empty() {
            return Err(UseCaseError::validation(
                "REASON_REQUIRED",
                "Suspension reason is required",
            ));
        }
        if reason.len() > 500 {
            return Err(UseCaseError::validation(
                "REASON_TOO_LONG",
                "Suspension reason must be at most 500 characters",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &SuspendClientCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        // Authorization handled in handler
        Ok(())
    }

    async fn execute(
        &self,
        command: SuspendClientCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ClientSuspended> {
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

impl<U: UnitOfWork> SuspendClientUseCase<U> {
    async fn prepare(
        &self,
        command: &SuspendClientCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Client, ClientSuspended), UseCaseError> {
        let reason = command.reason.trim();

        // Fetch existing client
        let mut client = self
            .client_repo
            .find_by_id(&command.client_id)
            .await
            .or_not_found(
                "CLIENT_NOT_FOUND",
                format!("Client with ID '{}' not found", command.client_id),
            )?;

        // Business rule: cannot suspend an inactive client
        if client.status == ClientStatus::Inactive {
            return Err(UseCaseError::business_rule(
                "CANNOT_SUSPEND_INACTIVE",
                "Cannot suspend an inactive client",
            ));
        }

        // Business rule: client must not already be suspended
        if client.status == ClientStatus::Suspended {
            return Err(UseCaseError::business_rule(
                "ALREADY_SUSPENDED",
                "Client is already suspended",
            ));
        }

        // Suspend the client
        client.suspend(reason);

        // Create domain event
        let event = ClientSuspended::new(ctx, &client.id, reason);
        Ok((client, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = SuspendClientCommand {
            client_id: "client-123".to_string(),
            reason: "Payment overdue".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("clientId"));
        assert!(json.contains("Payment overdue"));
    }
}
