//! Create Client Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};
use regex::Regex;

use crate::client::entity::Client;
use crate::client::repository::ClientRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
    unit_of_work::HasId,
};
use super::events::ClientCreated;

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

impl HasId for Client {
    fn id(&self) -> &str {
        &self.id
    }

    fn collection_name() -> &'static str {
        "tnt_clients"
    }
}

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

    pub async fn execute(
        &self,
        command: CreateClientCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ClientCreated> {
        // Validation: name is required
        let name = command.name.trim();
        if name.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "NAME_REQUIRED",
                "Client name is required",
            ));
        }
        if name.len() > 100 {
            return UseCaseResult::failure(UseCaseError::validation(
                "NAME_TOO_LONG",
                "Client name must be at most 100 characters",
            ));
        }

        // Validation: identifier is required and must match pattern
        let identifier = command.identifier.trim().to_lowercase();
        if identifier.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "IDENTIFIER_REQUIRED",
                "Client identifier is required",
            ));
        }
        if identifier.len() < 2 || identifier.len() > 50 {
            return UseCaseResult::failure(UseCaseError::validation(
                "INVALID_IDENTIFIER_LENGTH",
                "Client identifier must be between 2 and 50 characters",
            ));
        }
        if !identifier_pattern().is_match(&identifier) {
            return UseCaseResult::failure(UseCaseError::validation(
                "INVALID_IDENTIFIER_FORMAT",
                "Client identifier must be lowercase alphanumeric with hyphens, starting with a letter",
            ));
        }

        // Business rule: identifier must be unique
        if let Ok(Some(_)) = self.client_repo.find_by_identifier(&identifier).await {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "IDENTIFIER_EXISTS",
                format!("A client with identifier '{}' already exists", identifier),
            ));
        }

        // Create the client entity
        let client = Client::new(name, &identifier);

        // Create domain event
        let event = ClientCreated::new(
            &ctx,
            &client.id,
            &client.name,
            &client.identifier,
            None,
        );

        // Atomic commit: entity + event + audit log
        self.unit_of_work.commit(&client, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(Client::collection_name(), "tnt_clients");
    }
}
