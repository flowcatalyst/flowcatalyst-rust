//! Update Connection Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ConnectionUpdated;
use crate::connection::entity::ConnectionStatus;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ConnectionRepository;

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
    pub external_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ConnectionStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,
}

impl crate::usecase::AuditMasked for UpdateConnectionCommand {}

pub struct UpdateConnectionUseCase<U: UnitOfWork> {
    connection_repo: Arc<ConnectionRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> UpdateConnectionUseCase<U> {
    pub fn new(connection_repo: Arc<ConnectionRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            connection_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateConnectionUseCase<U> {
    type Command = UpdateConnectionCommand;
    type Event = ConnectionUpdated;

    async fn validate(&self, command: &UpdateConnectionCommand) -> Result<(), UseCaseError> {
        if command.connection_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CONNECTION_ID_REQUIRED",
                "Connection ID is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &UpdateConnectionCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateConnectionCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ConnectionUpdated> {
        let (connection, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&connection, &*self.connection_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> UpdateConnectionUseCase<U> {
    async fn prepare(
        &self,
        command: &UpdateConnectionCommand,
        ctx: &ExecutionContext,
    ) -> Result<(crate::Connection, ConnectionUpdated), UseCaseError> {
        let mut connection = self
            .connection_repo
            .find_by_id(&command.connection_id)
            .await
            .or_not_found(
                "CONNECTION_NOT_FOUND",
                format!("Connection with ID '{}' not found", command.connection_id),
            )?;

        // Apply selective updates
        if let Some(ref name) = command.name {
            connection.name = name.clone();
        }
        if let Some(ref desc) = command.description {
            connection.description = Some(desc.clone());
        }
        if let Some(ref ext_id) = command.external_id {
            connection.external_id = Some(ext_id.clone());
        }
        if let Some(ref sa_id) = command.service_account_id {
            connection.service_account_id = sa_id.clone();
        }
        match command.status {
            Some(ConnectionStatus::Active) => connection.activate(),
            Some(ConnectionStatus::Paused) => connection.pause(),
            None => {}
        }
        connection.updated_at = chrono::Utc::now();

        let event = ConnectionUpdated::new(ctx, &connection.id, &connection.name);
        Ok((connection, event))
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
            external_id: None,
            status: None,
            service_account_id: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("connectionId"));
    }
}
