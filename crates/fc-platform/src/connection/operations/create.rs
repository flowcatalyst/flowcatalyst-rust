//! Create Connection Use Case

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ConnectionCreated;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::Connection;
use crate::ConnectionRepository;
use crate::ServiceAccountRepository;

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
    pub service_account_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Who is creating it, for the signing-reach check (never serialised,
    /// so never in the audit log).
    #[serde(skip)]
    pub caller: Option<crate::shared::authorization_service::AuthContext>,
}

impl crate::usecase::AuditMasked for CreateConnectionCommand {}

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
        Self {
            connection_repo,
            service_account_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateConnectionUseCase<U> {
    type Command = CreateConnectionCommand;
    type Event = ConnectionCreated;

    async fn validate(&self, command: &CreateConnectionCommand) -> Result<(), UseCaseError> {
        let code = command.code.trim().to_lowercase();
        if code.is_empty() {
            return Err(UseCaseError::validation(
                "CODE_REQUIRED",
                "Connection code is required",
            ));
        }
        if !code_pattern().is_match(&code) {
            return Err(UseCaseError::validation(
                "INVALID_CODE_FORMAT", "Code must start with lowercase letter, contain only lowercase alphanumeric and hyphens",
            ));
        }

        let name = command.name.trim();
        if name.is_empty() {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "Connection name is required",
            ));
        }

        if command.service_account_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "SERVICE_ACCOUNT_ID_REQUIRED",
                "Service account ID is required",
            ));
        }

        Ok(())
    }

    async fn authorize(
        &self,
        _command: &CreateConnectionCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateConnectionCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ConnectionCreated> {
        let (connection, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&connection, &*self.connection_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> CreateConnectionUseCase<U> {
    async fn prepare(
        &self,
        command: &CreateConnectionCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Connection, ConnectionCreated), UseCaseError> {
        let code = command.code.trim().to_lowercase();
        let name = command.name.trim();

        // Validate service account exists
        self.service_account_repo
            .find_by_id(&command.service_account_id)
            .await
            .or_not_found(
                "SERVICE_ACCOUNT_NOT_FOUND",
                format!("Service account '{}' not found", command.service_account_id),
            )?;

        // The account must be one the caller may sign with (S7; Java
        // `CreateConnection`, b1ce6e55): every subscription on this
        // connection is delivered with it. `applicationCode`-style ownership
        // counts for nothing.
        crate::service_account::signing_reach::require_usable_signers(
            command.caller.as_ref(),
            &self.service_account_repo,
            &self.connection_repo,
            Some(&command.service_account_id),
            true,
            None,
            false,
        )
        .await?;

        // Uniqueness check (code + client_id scope)
        let existing = self
            .connection_repo
            .find_by_code_and_client(&code, command.client_id.as_deref())
            .await?;
        if existing.is_some() {
            return Err(UseCaseError::business_rule(
                "CONNECTION_CODE_EXISTS",
                format!(
                    "A connection with code '{}' already exists in this scope",
                    code
                ),
            ));
        }

        let mut connection = Connection::new(&code, name, &command.service_account_id);
        connection.description = command.description.clone();
        connection.client_id = command.client_id.clone();
        if let Some(ref ext_id) = command.external_id {
            connection.external_id = Some(ext_id.clone());
        }

        let event = ConnectionCreated::new(
            ctx,
            &connection.id,
            &connection.code,
            &connection.name,
            &connection.service_account_id,
            connection.client_id.as_deref(),
        );
        Ok((connection, event))
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
            service_account_id: "sa-123".to_string(),
            external_id: None,
            client_id: None,
            caller: None,
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
