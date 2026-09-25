//! Create Client Use Case

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ClientCreated;
use crate::client::entity::Client;
use crate::client::repository::ClientRepository;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};

/// Identifier format: lowercase alphanumeric with hyphens, 2-50 chars
fn identifier_pattern() -> &'static Regex {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"^[a-z][a-z0-9-]*[a-z0-9]$").unwrap())
}

/// Command for creating a new client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateClientCommand {
    /// Human-readable name (1-100 chars)
    pub name: String,

    /// Unique identifier/slug (lowercase alphanumeric with hyphens, 2-50 chars)
    pub identifier: String,
}

impl crate::usecase::AuditMasked for CreateClientCommand {}

/// Use case for creating a new client.
pub struct CreateClientUseCase<U: UnitOfWork> {
    client_repo: Arc<ClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateClientUseCase<U> {
    pub fn new(client_repo: Arc<ClientRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            client_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateClientUseCase<U> {
    type Command = CreateClientCommand;
    type Event = ClientCreated;

    async fn validate(&self, command: &CreateClientCommand) -> Result<(), UseCaseError> {
        let name = command.name.trim();
        if name.is_empty() {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "Client name is required",
            ));
        }
        if name.len() > 100 {
            return Err(UseCaseError::validation(
                "NAME_TOO_LONG",
                "Client name must be at most 100 characters",
            ));
        }

        let identifier = command.identifier.trim().to_lowercase();
        if identifier.is_empty() {
            return Err(UseCaseError::validation(
                "IDENTIFIER_REQUIRED",
                "Client identifier is required",
            ));
        }
        if identifier.len() < 2 || identifier.len() > 50 {
            return Err(UseCaseError::validation(
                "INVALID_IDENTIFIER_LENGTH",
                "Client identifier must be between 2 and 50 characters",
            ));
        }
        if !identifier_pattern().is_match(&identifier) {
            return Err(UseCaseError::validation(
                "INVALID_IDENTIFIER_FORMAT",
                "Client identifier must be lowercase alphanumeric with hyphens, starting with a letter",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &CreateClientCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        // Authorization handled in handler via require_anchor
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateClientCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ClientCreated> {
        let identifier = command.identifier.trim().to_lowercase();

        // Business rule: identifier must be unique
        let existing = match self.client_repo.find_by_identifier(&identifier).await {
            Ok(found) => found,
            Err(e) => return UseCaseResult::failure(e.into()),
        };
        if existing.is_some() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "IDENTIFIER_EXISTS",
                format!("A client with identifier '{}' already exists", identifier),
            ));
        }

        let client = Client::new(command.name.trim(), &identifier);

        let event = ClientCreated::new(&ctx, &client.id, &client.name, &client.identifier);

        self.unit_of_work
            .commit(&client, &*self.client_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::unit_of_work::HasId;

    #[test]
    fn test_command_serialization() {
        let cmd = CreateClientCommand {
            name: "Acme Corporation".to_string(),
            identifier: "acme-corp".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("Acme Corporation"));
        assert!(json.contains("acme-corp"));
    }

    #[test]
    fn test_identifier_pattern() {
        assert!(identifier_pattern().is_match("acme-corp"));
        assert!(identifier_pattern().is_match("test123"));
        assert!(identifier_pattern().is_match("my-client-2024"));
        assert!(!identifier_pattern().is_match("UPPERCASE"));
        assert!(!identifier_pattern().is_match("-starts-with-dash"));
        assert!(!identifier_pattern().is_match("ends-with-dash-"));
        assert!(!identifier_pattern().is_match("a")); // Too short
    }

    #[test]
    fn test_client_has_id() {
        let client = Client::new("Test", "test");
        assert!(!client.id().is_empty());
    }
}
