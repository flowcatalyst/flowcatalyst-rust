//! Delete Subscription Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::SubscriptionRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::SubscriptionDeleted;

/// Command for deleting a subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSubscriptionCommand {
    /// Subscription ID to delete
    pub subscription_id: String,
}

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

    pub async fn execute(
        &self,
        command: DeleteSubscriptionCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<SubscriptionDeleted> {
        // Validation: subscription_id is required
        if command.subscription_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "SUBSCRIPTION_ID_REQUIRED",
                "Subscription ID is required",
            ));
        }

        // Fetch existing subscription
        let subscription = match self.subscription_repo.find_by_id(&command.subscription_id).await {
            Ok(Some(s)) => s,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "SUBSCRIPTION_NOT_FOUND",
                    format!("Subscription with ID '{}' not found", command.subscription_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to fetch subscription: {}",
                    e
                )));
            }
        };

        // Create domain event
        let event = SubscriptionDeleted::new(&ctx, &subscription.id, &subscription.code);

        // Atomic commit with delete
        self.unit_of_work.commit_delete(&subscription, event, &command).await
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
