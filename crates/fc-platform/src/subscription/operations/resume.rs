//! Resume Subscription Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::SubscriptionResumed;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::Subscription;
use crate::SubscriptionRepository;
use crate::SubscriptionStatus;

/// Command for resuming a paused subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeSubscriptionCommand {
    /// Subscription ID to resume
    pub subscription_id: String,
}

impl crate::usecase::AuditMasked for ResumeSubscriptionCommand {}

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
}

#[async_trait]
impl<U: UnitOfWork> UseCase for ResumeSubscriptionUseCase<U> {
    type Command = ResumeSubscriptionCommand;
    type Event = SubscriptionResumed;

    async fn validate(&self, command: &ResumeSubscriptionCommand) -> Result<(), UseCaseError> {
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
        _command: &ResumeSubscriptionCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: ResumeSubscriptionCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<SubscriptionResumed> {
        let (subscription, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        // Atomic commit
        self.unit_of_work
            .commit(&subscription, &*self.subscription_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> ResumeSubscriptionUseCase<U> {
    async fn prepare(
        &self,
        command: &ResumeSubscriptionCommand,
        ctx: &ExecutionContext,
    ) -> Result<(Subscription, SubscriptionResumed), UseCaseError> {
        // Fetch existing subscription
        let mut subscription = self
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

        // Business rule: can only resume paused subscriptions
        if subscription.status == SubscriptionStatus::Active {
            return Err(UseCaseError::business_rule(
                "ALREADY_ACTIVE",
                "Subscription is already active",
            ));
        }

        // Resume the subscription
        subscription.resume();

        // Create domain event
        let event = SubscriptionResumed::new(ctx, &subscription.id, &subscription.code);
        Ok((subscription, event))
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
