//! Update Connection Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::ConnectionRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ConnectionUpdated;

/// Command for updating a connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConnectionCommand {
    pub connection_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,
}

pub struct UpdateConnectionUseCase<U: UnitOfWork> {
    connection_repo: Arc<ConnectionRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateConnectionUseCase<U> {
    pub fn new(connection_repo: Arc<ConnectionRepository>, unit_of_work: Arc<U>) -> Self {
        Self { connection_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: UpdateConnectionCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ConnectionUpdated> {
        if command.connection_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "CONNECTION_ID_REQUIRED", "Connection ID is required",
            ));
        }

        let mut connection = match self.connection_repo.find_by_id(&command.connection_id).await {
            Ok(Some(c)) => c,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "CONNECTION_NOT_FOUND",
                    format!("Connection with ID '{}' not found", command.connection_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch connection: {}", e
                )));
            }
        };

        // Apply selective updates
        if let Some(ref name) = command.name {
            connection.name = name.clone();
        }
        if let Some(ref desc) = command.description {
            connection.description = Some(desc.clone());
        }
        if let Some(ref endpoint) = command.endpoint {
            connection.endpoint = endpoint.clone();
        }
        if let Some(ref ext_id) = command.external_id {
            connection.external_id = Some(ext_id.clone());
        }
        if let Some(ref sa_id) = command.service_account_id {
            connection.service_account_id = sa_id.clone();
        }
        if let Some(ref status) = command.status {
            match status.to_uppercase().as_str() {
                "ACTIVE" => connection.activate(),
                "PAUSED" => connection.pause(),
                _ => {
                    return UseCaseResult::failure(UseCaseError::validation(
                        "INVALID_STATUS", "Status must be ACTIVE or PAUSED",
                    ));
                }
            }
        }
        connection.updated_at = chrono::Utc::now();

        let event = ConnectionUpdated::new(
            &ctx,
            &connection.id,
            &connection.code,
            command.name.as_deref(),
            command.endpoint.as_deref(),
            command.status.as_deref(),
        );

        self.unit_of_work.commit(&connection, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = UpdateConnectionCommand {
            connection_id: "conn-123".to_string(),
            name: Some("Updated Name".to_string()),
            description: None,
            endpoint: None,
            external_id: None,
            status: None,
            service_account_id: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("connectionId"));
    }
}
