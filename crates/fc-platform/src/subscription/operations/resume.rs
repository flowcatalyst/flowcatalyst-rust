//! Resume Subscription Use Case

use std::sync::Arc;
use serde::{Deserialize, Serialize};

use crate::SubscriptionStatus;
use crate::SubscriptionRepository;
use crate::usecase::{
    ExecutionContext, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::SubscriptionResumed;

/// Command for resuming a paused subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeSubscriptionCommand {
    /// Subscription ID to resume
    pub subscription_id: String,
}

/// Use case for resuming a paused subscription.
pub struct ResumeSubscriptionUseCase<U: UnitOfWork> {
    subscription_repo: Arc<SubscriptionRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> ResumeSubscriptionUseCase<U> {
    pub fn new(subscription_repo: Arc<SubscriptionRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            subscription_repo,
            unit_of_work,
        }
    }

    pub async fn execute(
        &self,
        command: ResumeSubscriptionCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<SubscriptionResumed> {
        // Validation: subscription_id is required
        if command.subscription_id.trim().is_empty() {
            return UseCaseResult::failure(UseCaseError::validation(
                "SUBSCRIPTION_ID_REQUIRED",
                "Subscription ID is required",
            ));
        }

        // Fetch existing subscription
        let mut subscription = match self.subscription_repo.find_by_id(&command.subscription_id).await {
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

        // Business rule: can only resume paused subscriptions
        if subscription.status == SubscriptionStatus::Active {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "ALREADY_ACTIVE",
                "Subscription is already active",
            ));
        }

        if subscription.status == SubscriptionStatus::Archived {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "CANNOT_RESUME_ARCHIVED",
                "Cannot resume an archived subscription",
            ));
        }

        // Resume the subscription
        subscription.resume();

        // Create domain event
        let event = SubscriptionResumed::new(&ctx, &subscription.id, &subscription.code);

        // Atomic commit
        self.unit_of_work.commit(&subscription, event, &command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = ResumeSubscriptionCommand {
            subscription_id: "sub-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("subscriptionId"));
    }
}
