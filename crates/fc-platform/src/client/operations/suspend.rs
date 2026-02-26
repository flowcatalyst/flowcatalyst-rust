//! Suspend Client Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::client::entity::ClientStatus;
use crate::client::repository::ClientRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ClientSuspended;

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

    pub async fn execute(
        &self,
        command: SuspendClientCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ClientSuspended> {
        // Validation: client_id is required
        if command.client_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "CLIENT_ID_REQUIRED",
                "Client ID is required",
            ));
        }

        // Validation: reason is required
        let reason = command.reason.trim();
        if reason.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "REASON_REQUIRED",
                "Suspension reason is required",
            ));
        }
        if reason.len() > 500 {
            return UseCaseResult::failure(UseCaseError::validation(
                "REASON_TOO_LONG",
                "Suspension reason must be at most 500 characters",
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

        // Business rule: cannot suspend a deleted client
        if client.status == ClientStatus::Deleted {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "CANNOT_SUSPEND_DELETED",
                "Cannot suspend a deleted client",
            ));
        }

        // Business rule: client must not already be suspended
        if client.status == ClientStatus::Suspended {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "ALREADY_SUSPENDED",
                "Client is already suspended",
            ));
        }

        // Suspend the client
        client.suspend(reason);

        // Create domain event
        let event = ClientSuspended::new(&ctx, &client.id, reason);

        // Atomic commit
        self.unit_of_work.commit(&client, event, &command).await
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
