//! Delete Connection Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ConnectionDeleted;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ConnectionRepository;
use crate::SubscriptionRepository;

/// Command for deleting a connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteConnectionCommand {
    pub connection_id: String,
}

impl crate::usecase::AuditMasked for DeleteConnectionCommand {}

pub struct DeleteConnectionUseCase<U: UnitOfWork> {
    connection_repo: Arc<ConnectionRepository>,
    subscription_repo: Arc<SubscriptionRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteConnectionUseCase<U> {
    pub fn new(
        connection_repo: Arc<ConnectionRepository>,
        subscription_repo: Arc<SubscriptionRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            connection_repo,
            subscription_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteConnectionUseCase<U> {
    type Command = DeleteConnectionCommand;
    type Event = ConnectionDeleted;

    async fn validate(&self, command: &DeleteConnectionCommand) -> Result<(), UseCaseError> {
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
        _command: &DeleteConnectionCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteConnectionCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ConnectionDeleted> {
        let (connection, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit_delete(&connection, &*self.connection_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> DeleteConnectionUseCase<U> {
    async fn prepare(
        &self,
        command: &DeleteConnectionCommand,
        ctx: &ExecutionContext,
    ) -> Result<(crate::Connection, ConnectionDeleted), UseCaseError> {
        let connection = self
            .connection_repo
            .find_by_id(&command.connection_id)
            .await
            .or_not_found(
                "CONNECTION_NOT_FOUND",
                format!("Connection with ID '{}' not found", command.connection_id),
            )?;

        // Business rule: cannot delete if subscriptions reference this connection
        if self
            .subscription_repo
            .exists_by_connection_id(&connection.id)
            .await?
        {
            return Err(UseCaseError::business_rule(
                "HAS_SUBSCRIPTIONS",
                "Cannot delete a connection that has subscriptions. Remove all subscriptions first.",
            ));
        }

        let event = ConnectionDeleted::new(
            ctx,
            &connection.id,
            &connection.code,
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
        let cmd = DeleteConnectionCommand {
            connection_id: "conn-123".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("connectionId"));
    }
}
