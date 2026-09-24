//! Delete Subscription Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::SubscriptionDeleted;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::Subscription;
use crate::SubscriptionRepository;

/// Command for deleting a subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSubscriptionCommand {
    /// Subscription ID to delete
    pub subscription_id: String,
}

impl crate::usecase::AuditMasked for DeleteSubscriptionCommand {}

/// Use case for deleting a subscription.
pub struct DeleteSubscriptionUseCase<U: UnitOfWork> {
    subscription_repo: Arc<SubscriptionRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> DeleteSubscriptionUseCase<U> {
    pub fn new(subscription_repo: Arc<SubscriptionRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            subscription_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for DeleteSubscriptionUseCase<U> {
    type Command = DeleteSubscriptionCommand;
    type Event = SubscriptionDeleted;

    async fn validate(&self, command: &DeleteSubscriptionCommand) -> Result<(), UseCaseError> {
        if command.subscription_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "SUBSCRIPTION_ID_REQUIRED",
                "Subscription ID is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &DeleteSubscriptionCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: DeleteSubscriptionCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<SubscriptionDeleted> {
        let (subscription, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // Atomic commit with delete
        self.unit_of_work
            .commit_delete(&subscription, &*self.subscription_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> DeleteSubscriptionUseCase<U> {
    async fn prepare(
        &self,
        command: &DeleteSubscriptionCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Subscription, SubscriptionDeleted), UseCaseError> {
        // Fetch existing subscription
        let subscription = self
            .subscription_repo
            .find_by_id(&command.subscription_id)
            .await
            .or_not_found(
                "SUBSCRIPTION_NOT_FOUND",
                format!(
                    "Subscription with ID '{}' not found",
                    command.subscription_id
                ),
            )?;

        // Create domain event
        let event = SubscriptionDeleted::new(ctx, &subscription.id, &subscription.code);
        Ok((subscription, event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = DeleteSubscriptionCommand {
            subscription_id: "sub-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("subscriptionId"));
    }
}
