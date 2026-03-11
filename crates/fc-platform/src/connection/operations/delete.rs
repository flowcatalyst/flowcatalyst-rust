//! Delete Connection Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::ConnectionRepository;
use crate::SubscriptionRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ConnectionDeleted;

/// Command for deleting a connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteConnectionCommand {
    pub connection_id: String,
}

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
        Self { connection_repo, subscription_repo, unit_of_work }
    }

    pub async fn execute(
        &self,
        command: DeleteConnectionCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ConnectionDeleted> {
        if command.connection_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "CONNECTION_ID_REQUIRED", "Connection ID is required",
            ));
        }

        let connection = match self.connection_repo.find_by_id(&command.connection_id).await {
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

        // Business rule: cannot delete if subscriptions reference this connection
        match self.subscription_repo.exists_by_connection_id(&connection.id).await {
            Ok(true) => {
                return UseCaseResult::failure(UseCaseError::business_rule(
                    "HAS_SUBSCRIPTIONS",
                    "Cannot delete a connection that has subscriptions. Remove all subscriptions first.",
                ));
            }
            Ok(false) => {}
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to check subscriptions: {}", e
                )));
            }
        }

        let event = ConnectionDeleted::new(
            &ctx,
            &connection.id,
            &connection.code,
            connection.client_id.as_deref(),
        );

        self.unit_of_work.commit_delete(&connection, event, &command).await
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
