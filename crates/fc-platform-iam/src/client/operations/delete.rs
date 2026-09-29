//! Delete Client Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ClientDeleted;
use crate::client::repository::ClientRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for deleting a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteClientCommand {
    pub client_id: String,
}

impl AuditMasked for DeleteClientCommand {}

pub struct DeleteClientUseCase<U: UnitOfWork> {
    client_repo: Arc<ClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteClientUseCase<U> {
    pub fn new(client_repo: Arc<ClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            client_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteClientUseCase<U> {
    type Command = DeleteClientCommand;
    type Event = ClientDeleted;

    async fn validate(&self, command: &DeleteClientCommand) -> Result<(), UseCaseError> {
        if command.client_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CLIENT_ID_REQUIRED",
                "Client ID is required",
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
        _command: &DeleteClientCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: DeleteClientCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ClientDeleted>, UseCaseError> {
        let client = self
            .client_repo
            .find_by_id(&command.client_id)
            .await
            .or_not_found(
                "CLIENT_NOT_FOUND",
                format!("Client with ID '{}' not found", command.client_id),
            )?;

        // Business rule: refuse when principals still have this as home client.
        // `iam_principals.client_id` is a code-enforced reference (no DB-level FK).
        // Silently orphaning a user's home client would change their scope
        // without explicit action — force the admin to migrate them first.
        let home_principals = self
            .client_repo
            .count_home_principals(client.id.as_str())
            .await?;
        if home_principals > 0 {
            return Err(UseCaseError::business_rule(
                "CLIENT_HAS_PRINCIPALS",
                format!(
                    "Cannot delete client '{}' — {} principal(s) have it as their home client. \
                     Migrate those principals before deleting.",
                    client.identifier, home_principals,
                ),
            ));
        }

        // Business rules: refuse when any code-enforced reference still
        // points at this client. None of these have DB-level FKs — each
        // must be explicitly unwired before deletion.
        let grants = self
            .client_repo
            .count_access_grants(client.id.as_str())
            .await?;
        let configs = self
            .client_repo
            .count_client_configs(client.id.as_str())
            .await?;

        let refs = [("access grants", grants), ("application configs", configs)];
        let blockers: Vec<String> = refs
            .iter()
            .filter(|(_, n)| *n > 0)
            .map(|(label, n)| format!("{n} {label}"))
            .collect();
        if !blockers.is_empty() {
            return Err(UseCaseError::business_rule(
                "CLIENT_HAS_REFERENCES",
                format!(
                    "Cannot delete client '{}' — {} still reference it. \
                     Remove those before deleting.",
                    client.identifier,
                    blockers.join(", "),
                ),
            ));
        }

        let event = ClientDeleted::new(&ctx, client.id.as_str(), &client.identifier);

        self.unit_of_work
            .commit_delete(&client, &*self.client_repo, event, &command)
            .await
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
