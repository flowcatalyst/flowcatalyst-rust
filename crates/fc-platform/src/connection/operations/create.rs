//! Create Connection Use Case

use std::sync::Arc;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::Connection;
use crate::ConnectionRepository;
use crate::ServiceAccountRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ConnectionCreated;

fn code_pattern() -> &'static Regex {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"^[a-z][a-z0-9-]*$").unwrap())
}

/// Command for creating a new connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateConnectionCommand {
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub endpoint: String,
    pub service_account_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

pub struct CreateConnectionUseCase<U: UnitOfWork> {
    connection_repo: Arc<ConnectionRepository>,
    service_account_repo: Arc<ServiceAccountRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateConnectionUseCase<U> {
    pub fn new(
        connection_repo: Arc<ConnectionRepository>,
        service_account_repo: Arc<ServiceAccountRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self { connection_repo, service_account_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: CreateConnectionCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ConnectionCreated> {
        let code = command.code.trim().to_lowercase();
        if code.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "CODE_REQUIRED", "Connection code is required",
            ));
        }
        if !code_pattern().is_match(&code) {
            return UseCaseResult::failure(UseCaseError::validation(
                "INVALID_CODE_FORMAT", "Code must start with lowercase letter, contain only lowercase alphanumeric and hyphens",
            ));
        }

        let name = command.name.trim();
        if name.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "NAME_REQUIRED", "Connection name is required",
            ));
        }

        let endpoint = command.endpoint.trim();
        if endpoint.is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "ENDPOINT_REQUIRED", "Endpoint is required",
            ));
        }

        if command.service_account_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "SERVICE_ACCOUNT_ID_REQUIRED", "Service account ID is required",
            ));
        }

        // Validate service account exists
        match self.service_account_repo.find_by_id(&command.service_account_id).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "SERVICE_ACCOUNT_NOT_FOUND",
                    format!("Service account '{}' not found", command.service_account_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to validate service account: {}", e
                )));
            }
        }

        // Uniqueness check (code + client_id scope)
        let existing = self.connection_repo
            .find_by_code_and_client(&code, command.client_id.as_deref())
            .await;
        if let Ok(Some(_)) = existing {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "CONNECTION_CODE_EXISTS",
                format!("A connection with code '{}' already exists in this scope", code),
            ));
        }

        let mut connection = Connection::new(&code, name, endpoint, &command.service_account_id);
        connection.description = command.description.clone();
        connection.client_id = command.client_id.clone();
        if let Some(ref ext_id) = command.external_id {
            connection.external_id = Some(ext_id.clone());
        }

        let event = ConnectionCreated::new(
            &ctx,
            &connection.id,
            &connection.code,
            &connection.name,
            &connection.endpoint,
            &connection.service_account_id,
            connection.client_id.as_deref(),
        );

        self.unit_of_work.commit(&connection, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = CreateConnectionCommand {
            code: "my-webhook".to_string(),
            name: "My Webhook".to_string(),
            description: None,
            endpoint: "https://example.com/webhook".to_string(),
            service_account_id: "sa-123".to_string(),
            external_id: None,
            client_id: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("my-webhook"));
    }

    #[test]
    fn test_code_pattern() {
        let pattern = code_pattern();
        assert!(pattern.is_match("my-webhook"));
        assert!(pattern.is_match("a"));
        assert!(!pattern.is_match("My-Webhook"));
        assert!(!pattern.is_match("-webhook"));
        assert!(!pattern.is_match("123webhook"));
    }
}
