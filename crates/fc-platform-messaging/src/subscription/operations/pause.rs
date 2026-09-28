//! Pause Subscription Use Case

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::SubscriptionPaused;
use crate::subscription::repository::SubscriptionRepository;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for pausing a subscription.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseSubscriptionCommand {
    /// Subscription ID to pause
    pub subscription_id: String,
}

impl fc_platform_core::usecase::AuditMasked for PauseSubscriptionCommand {}

/// Use case for pausing a subscription.
pub struct PauseSubscriptionUseCase<U: UnitOfWork> {
    subscription_repo: Arc<SubscriptionRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> PauseSubscriptionUseCase<U> {
    pub fn new(subscription_repo: Arc<SubscriptionRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            subscription_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for PauseSubscriptionUseCase<U> {
    type Command = PauseSubscriptionCommand;
    type Event = SubscriptionPaused;

    async fn validate(&self, command: &PauseSubscriptionCommand) -> Result<(), UseCaseError> {
        if command.subscription_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "SUBSCRIPTION_ID_REQUIRED",
                "Subscription ID is required",
            ));
        }
        Ok(())
    }

    /// Go `CheckScopeAccess` on the stored subscription (Go checks it post-load): a
    /// client's subscription needs that client, a platform one anchor scope (403
    /// `SCOPE_FORBIDDEN`). A missing subscription is `execute`'s 404.
    async fn authorize(
        &self,
        command: &PauseSubscriptionCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        if let Some(target) = self
            .subscription_repo
            .find_by_id(&command.subscription_id)
            .await?
        {
            fc_platform_core::shared::caller_reach::check_scope_access(
                ctx.caller(),
                target.client_id.as_deref(),
            )?;
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: PauseSubscriptionCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<SubscriptionPaused>, UseCaseError> {
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

        // Unconditional, as Go: a repeat is a no-op write that still
        // records the event (`subscription/operations/pause.go`).

        // Pause the subscription
        subscription.pause();

        // Create domain event
        let event = SubscriptionPaused::new(&ctx, &subscription.id);

        // Atomic commit
        self.unit_of_work
            .commit(&subscription, &*self.subscription_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = PauseSubscriptionCommand {
            subscription_id: "sub-123".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("subscriptionId"));
    }
}
